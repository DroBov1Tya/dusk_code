//! The bot as a platform of the core: started as `duskcode --platform telegram`, it reads
//! the core's messages on stdin and writes the owner's input on stdout, one JSON object
//! a line; the core's `plugins::platform` describes them. Its settings come from the
//! plugin's: `token`, from @BotFather.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};

use crate::{BotCommand, BotEvent, FromBot, TelegramBot, ToBot};

const OWNER_FILE: &str = "owner";

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FromCore {
    Answer {
        text: String,
    },
    Question {
        question: String,
        options: Vec<String>,
        multiple: bool,
    },
    Approval {
        id: u64,
        text: String,
    },
    Reply {
        text: String,
    },
    Menu {
        command: String,
        title: String,
        options: Vec<String>,
    },
    Image {
        path: PathBuf,
        mime_type: String,
        caption: Option<String>,
    },
    Working {
        on: bool,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToCore {
    Message {
        text: String,
    },
    Chosen {
        options: Vec<usize>,
    },
    Approval {
        id: u64,
        allow: bool,
    },
    Command {
        name: &'static str,
        argument: Option<String>,
    },
    Status {
        text: String,
    },
}

pub async fn run() -> Result<()> {
    let settings: BTreeMap<String, String> = std::env::var("DUSKCODE_PLUGIN_SETTINGS")
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let token = settings
        .get("token")
        .filter(|token| !token.trim().is_empty())
        .context("the telegram plugin needs its token setting")?;
    let data = std::env::var_os("DUSKCODE_PLUGIN_DATA").map(PathBuf::from);
    let mut bot = TelegramBot::connect(token.clone(), 0).await?;
    if let Some((user, chat, name)) = data.as_deref().and_then(read_owner) {
        bot.restore_owner(user, chat, name);
    }
    let (events, mut received) = mpsc::unbounded_channel();
    bot.start(events);
    let mut stdout = tokio::io::stdout();
    write(&mut stdout, &status(&bot)).await?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            line = lines.next_line() => {
                // The core closed the pipe: it stops this platform.
                let Some(line) = line? else { return Ok(()) };
                if let Ok(message) = serde_json::from_str::<FromCore>(&line) {
                    bot.send(to_bot(message));
                }
            }
            event = received.recv() => match event {
                Some((_, BotEvent::Paired { .. })) => {
                    if let (Some(data), Some(owner)) = (&data, bot.owner_ids()) {
                        let _ = write_owner(data, &owner);
                    }
                    write(&mut stdout, &status(&bot)).await?;
                }
                Some((_, BotEvent::Stopped { reason })) => bail!("the bot stopped: {reason}"),
                Some((_, BotEvent::Input(input))) => write(&mut stdout, &to_core(input)).await?,
                None => return Ok(()),
            },
        }
    }
}

fn status(bot: &TelegramBot) -> ToCore {
    let text = match bot.owner() {
        Some(owner) => format!("@{} paired with {owner}", bot.username()),
        None => format!(
            "send /start {} to @{} within 10 minutes to pair",
            bot.code(),
            bot.username()
        ),
    };
    ToCore::Status { text }
}

fn to_bot(message: FromCore) -> ToBot {
    match message {
        FromCore::Answer { text } => ToBot::Answer(text),
        FromCore::Question {
            question,
            options,
            multiple,
        } => ToBot::Question {
            question,
            options,
            multiple,
        },
        FromCore::Approval { id, text } => ToBot::Approval { id, text },
        FromCore::Reply { text } => ToBot::Reply(text),
        FromCore::Menu {
            command,
            title,
            options,
        } => ToBot::Menu {
            command,
            title,
            options,
        },
        FromCore::Image {
            path,
            mime_type,
            caption,
        } => ToBot::Image {
            path,
            mime_type,
            caption,
        },
        FromCore::Working { on } => ToBot::Working(on),
    }
}

fn to_core(input: FromBot) -> ToCore {
    match input {
        FromBot::Message(text) => ToCore::Message { text },
        FromBot::Chosen(options) => ToCore::Chosen { options },
        FromBot::Approval { id, allow } => ToCore::Approval { id, allow },
        FromBot::Command(command) => {
            let (name, argument) = match command {
                BotCommand::Stop => ("stop", None),
                BotCommand::Resume => ("resume", None),
                BotCommand::Status => ("status", None),
                BotCommand::Approve => ("approve", None),
                BotCommand::Deny => ("deny", None),
                BotCommand::Mode(argument) => ("mode", argument),
                BotCommand::Model(argument) => ("model", argument),
                BotCommand::Compact(argument) => ("compact", argument),
                BotCommand::Goal(argument) => ("goal", argument),
            };
            ToCore::Command { name, argument }
        }
    }
}

async fn write(stdout: &mut tokio::io::Stdout, message: &ToCore) -> Result<()> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

/// `user chat name` on one line.
fn read_owner(data: &std::path::Path) -> Option<(u64, i64, String)> {
    let text = std::fs::read_to_string(data.join(OWNER_FILE)).ok()?;
    let mut parts = text.trim().splitn(3, ' ');
    let user = parts.next()?.parse().ok()?;
    let chat = parts.next()?.parse().ok()?;
    Some((user, chat, parts.next().unwrap_or_default().to_owned()))
}

fn write_owner(data: &std::path::Path, (user, chat, name): &(u64, i64, String)) -> Result<()> {
    use std::io::Write;
    let name = name.replace('\n', " ");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(data.join(OWNER_FILE))?;
    file.write_all(format!("{user} {chat} {name}\n").as_bytes())?;
    Ok(())
}
