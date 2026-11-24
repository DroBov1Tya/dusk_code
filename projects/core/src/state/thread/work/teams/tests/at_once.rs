//! Members at once: set by the user while the team runs, 1 to the setting.

use super::*;

#[tokio::test]
async fn members_at_once_change_within_the_setting_while_the_team_runs() {
    let state = chats().await;
    assert!(matches!(
        state.start_team(LEAD, 0, START).await,
        Err(TeamError::PastCeiling { .. })
    ));
    assert!(matches!(
        state.start_team(LEAD, 9, START).await,
        Err(TeamError::PastCeiling { .. })
    ));
    assert!(state.team_run(LEAD).await.unwrap().is_none());
    state.start_team(LEAD, plain(), START).await.unwrap();

    for (asked, ceiling) in [(0, 4), (5, 4)] {
        assert!(matches!(
            state.set_team_at_once("m0", asked, ceiling).await,
            Err(TeamError::PastCeiling { ceiling: told }) if told == ceiling
        ));
    }
    assert_eq!(
        state.team_run(LEAD).await.unwrap().unwrap().at_once,
        plain()
    );
    for asked in [1, 4] {
        let run = state.set_team_at_once("m0", asked, 4).await.unwrap();
        assert_eq!((run.thread_id.as_str(), run.at_once), (LEAD, asked));
    }
    let team = state.team(LEAD, START).await.unwrap().unwrap();
    assert_eq!(team.at_once, 4);
}
