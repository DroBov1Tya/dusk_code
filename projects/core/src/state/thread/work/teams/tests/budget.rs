//! Who may change a budget, part by part.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Taken,
    LeadRaises(BudgetPart),
    PastCeiling(BudgetPart),
}

#[tokio::test]
async fn the_lead_only_lowers_and_the_user_raises_to_the_ceiling() {
    use BudgetBy::{Lead, User};
    use BudgetPart::{AtOnce, Minutes, Steps, Tokens};
    use Expect::{LeadRaises, PastCeiling, Taken};
    let state = chats().await;
    for (asked, part) in [
        (budget(5, 400, 4_000_000, 120), AtOnce),
        (budget(3, 501, 4_000_000, 120), Steps),
        (budget(3, 400, 5_000_001, 120), Tokens),
        (budget(3, 400, 4_000_000, 181), Minutes),
    ] {
        match state.start_team(LEAD, asked, ceilings(), START).await {
            Err(TeamError::PastCeiling { part: refused, .. }) if refused == part => {}
            other => panic!("a start past the {part:?} ceiling: {other:?}"),
        }
    }
    assert!(state.team_run(LEAD).await.unwrap().is_none());
    state
        .start_team(LEAD, plain(), ceilings(), START)
        .await
        .unwrap();

    let low = TeamCeilings::new(budget(1, 10, 100_000, 5));
    let table = [
        (Lead, budget(2, 300, 3_000_000, 60), ceilings(), Taken),
        (
            Lead,
            budget(3, 300, 3_000_000, 60),
            ceilings(),
            LeadRaises(AtOnce),
        ),
        (
            Lead,
            budget(2, 301, 3_000_000, 60),
            ceilings(),
            LeadRaises(Steps),
        ),
        (
            Lead,
            budget(2, 300, 3_000_001, 60),
            ceilings(),
            LeadRaises(Tokens),
        ),
        (
            Lead,
            budget(2, 300, 3_000_000, 61),
            ceilings(),
            LeadRaises(Minutes),
        ),
        (Lead, budget(2, 300, 3_000_000, 60), ceilings(), Taken),
        (User, budget(4, 500, 5_000_000, 180), ceilings(), Taken),
        (
            User,
            budget(5, 500, 5_000_000, 180),
            ceilings(),
            PastCeiling(AtOnce),
        ),
        (
            User,
            budget(4, 501, 5_000_000, 180),
            ceilings(),
            PastCeiling(Steps),
        ),
        (
            User,
            budget(4, 500, 5_000_001, 180),
            ceilings(),
            PastCeiling(Tokens),
        ),
        (
            User,
            budget(4, 500, 5_000_000, 181),
            ceilings(),
            PastCeiling(Minutes),
        ),
        // A ceiling lowered under the running budget still lets it go down.
        (User, budget(2, 100, 1_000_000, 30), low, Taken),
        (
            User,
            budget(3, 100, 1_000_000, 30),
            low,
            PastCeiling(AtOnce),
        ),
    ];
    let mut current = plain();
    for (by, asked, ceilings, expect) in table {
        let answer = state.set_team_budget(LEAD, by, asked, ceilings).await;
        let got = match answer {
            Ok(_) => Taken,
            Err(TeamError::LeadRaises(part)) => LeadRaises(part),
            Err(TeamError::PastCeiling { part, .. }) => PastCeiling(part),
            Err(other) => panic!("{by:?} {asked:?}: {other}"),
        };
        assert_eq!(got, expect, "{by:?} {asked:?}");
        if got == Taken {
            current = asked;
        }
        let run = state.team_run(LEAD).await.unwrap().unwrap();
        assert_eq!(run.budget, current, "{by:?} {asked:?}");
        let minutes = i64::from(current.minutes());
        assert_eq!(run.deadline, START + minutes * MINUTE);
    }
}
