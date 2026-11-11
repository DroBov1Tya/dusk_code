//! The WebSocket to the core. Requests go out as frames numbered by the page, the
//! responses find their waiting call by that number, and events and status are handed to
//! the handlers the app gave when it connected.

use dusk_protocol::client::{Event, Request, RequestFrame, Response, ServerFrame, Status};
use futures::channel::oneshot;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{CloseEvent, MessageEvent, WebSocket};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conn {
    Connecting,
    Online,
    /// The stream broke; the page tries again.
    Lost,
}

/// What the app does with what the core sends.
pub struct Handlers {
    pub on_event: Box<dyn Fn(Event)>,
    pub on_status: Box<dyn Fn(Status)>,
    pub on_conn: Box<dyn Fn(Conn)>,
}

struct Link {
    socket: Option<WebSocket>,
    next: u64,
    pending: HashMap<u64, oneshot::Sender<Response>>,
    handlers: Rc<Handlers>,
}

thread_local! {
    static LINK: RefCell<Option<Link>> = const { RefCell::new(None) };
}

const RETRY_MS: i32 = 2000;

pub fn connect(handlers: Handlers) {
    LINK.with(|link| {
        *link.borrow_mut() = Some(Link {
            socket: None,
            next: 1,
            pending: HashMap::new(),
            handlers: Rc::new(handlers),
        });
    });
    open();
}

fn handlers() -> Option<Rc<Handlers>> {
    LINK.with(|link| link.borrow().as_ref().map(|link| Rc::clone(&link.handlers)))
}

fn open() {
    let Some(handlers) = handlers() else { return };
    (handlers.on_conn)(Conn::Connecting);
    let Some(url) = socket_url() else { return };
    let Ok(socket) = WebSocket::new(&url) else {
        retry_later();
        return;
    };

    let on_open = {
        let handlers = Rc::clone(&handlers);
        Closure::<dyn FnMut()>::new(move || (handlers.on_conn)(Conn::Online))
    };
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let on_message = {
        let handlers = Rc::clone(&handlers);
        Closure::<dyn FnMut(MessageEvent)>::new(move |message: MessageEvent| {
            let Some(text) = message.data().as_string() else {
                return;
            };
            match serde_json::from_str::<ServerFrame>(&text) {
                Ok(frame) => deliver(&handlers, frame),
                Err(error) => web_sys::console::warn_1(&format!("bad frame: {error}").into()),
            }
        })
    };
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| {
        // Calls still waiting will never be answered on this stream.
        LINK.with(|link| {
            if let Some(link) = link.borrow_mut().as_mut() {
                link.socket = None;
                link.pending.clear();
            }
        });
        (handlers.on_conn)(Conn::Lost);
        retry_later();
    });
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    LINK.with(|link| {
        if let Some(link) = link.borrow_mut().as_mut() {
            link.socket = Some(socket);
        }
    });
}

fn deliver(handlers: &Handlers, frame: ServerFrame) {
    match frame {
        ServerFrame::Response { id, response } => {
            let waiting = LINK.with(|link| {
                link.borrow_mut()
                    .as_mut()
                    .and_then(|link| link.pending.remove(&id))
            });
            if let Some(waiting) = waiting {
                let _ = waiting.send(response);
            }
        }
        ServerFrame::Event(event) => (handlers.on_event)(event),
        ServerFrame::Status(status) => (handlers.on_status)(*status),
    }
}

fn retry_later() {
    let retry = Closure::once_into_js(open);
    if let Some(window) = web_sys::window() {
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(retry.unchecked_ref(), RETRY_MS);
    }
}

fn socket_url() -> Option<String> {
    let location = web_sys::window()?.location();
    let scheme = if location.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    Some(format!("{scheme}://{}/api/ws", location.host().ok()?))
}

/// Sends a request and waits for its response. The error is the core's own text.
pub async fn request(request: Request) -> Result<Response, String> {
    let (reply, answer) = oneshot::channel();
    let sent = LINK.with(|link| {
        let mut link = link.borrow_mut();
        let link = link.as_mut()?;
        let socket = link.socket.as_ref()?;
        let id = link.next;
        link.next += 1;
        let text = serde_json::to_string(&RequestFrame { id, request }).ok()?;
        socket.send_with_str(&text).ok()?;
        link.pending.insert(id, reply);
        Some(())
    });
    if sent.is_none() {
        return Err("Not connected to the core.".to_owned());
    }
    match answer.await {
        Ok(Response::Error(error)) => Err(error),
        Ok(response) => Ok(response),
        Err(_) => Err("The connection was lost.".to_owned()),
    }
}
