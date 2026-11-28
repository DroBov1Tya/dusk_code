//! Giving the events of open threads to the clients whose scope covers them.

use super::{ClientId, Subscribers};
use dusk_protocol::client::{Event, ProjectRequest, Request};
use dusk_protocol::project::RepositoryStatus;

/// The thread an event is about; none for what belongs to one client, such as a
/// terminal's bytes, which is never given to all.
fn thread_of(event: &Event) -> Option<&str> {
    match event {
        Event::Thread { thread_id, .. } | Event::Ended { thread_id } => Some(thread_id),
        Event::Terminal { .. }
        | Event::TerminalClosed { .. }
        | Event::TerminalOpened { .. }
        | Event::PluginView { .. }
        | Event::PluginFrame { .. }
        | Event::PluginNotice { .. }
        | Event::GitChanged { .. }
        | Event::RepositoryStatus { .. }
        | Event::CoreStopping { .. } => None,
    }
}

/// What a client without full access is told when the core stops: it never learns
/// which other client or reason stopped it.
const GENERIC_STOP: &str = "the core is stopping";

/// Tells every attached client that the core is about to stop, whatever its scope. Only
/// full-access clients read `reason`.
pub(crate) fn stopping(subscribers: &Subscribers, reason: &str) {
    let own = Event::CoreStopping {
        reason: reason.to_owned(),
    };
    let other = Event::CoreStopping {
        reason: GENERIC_STOP.to_owned(),
    };
    subscribers.lock().retain(|subscriber| {
        let event = match subscriber.scope.is_full() {
            true => &own,
            false => &other,
        };
        subscriber.queue.send(event.clone()).is_ok()
    });
}

/// Gives a thread's event to every attached client whose scope covers the thread,
/// dropping the queues of the gone ones.
pub(super) fn publish(subscribers: &Subscribers, event: Event) {
    let Some(thread_id) = thread_of(&event) else {
        return;
    };
    subscribers.lock().retain(|subscriber| {
        !subscriber.scope.covers(thread_id) || subscriber.queue.send(event.clone()).is_ok()
    });
}

/// Tells every client that may read projects that one's repository changed.
pub(super) fn git_changed(subscribers: &Subscribers, project_id: &str) {
    let event = Event::GitChanged {
        project_id: project_id.to_owned(),
    };
    to_project_readers(subscribers, project_id, event);
}

/// Gives a project's new repository status to every client that may read projects.
pub(super) fn repository_status(
    subscribers: &Subscribers,
    project_id: &str,
    status: &RepositoryStatus,
) {
    let event = Event::RepositoryStatus {
        project_id: project_id.to_owned(),
        status: Box::new(status.clone()),
    };
    to_project_readers(subscribers, project_id, event);
}

fn to_project_readers(subscribers: &Subscribers, project_id: &str, event: Event) {
    let reads = Request::Project(ProjectRequest::Git {
        id: project_id.to_owned(),
    });
    subscribers.lock().retain(|subscriber| {
        !subscriber.scope.permits(&reads) || subscriber.queue.send(event.clone()).is_ok()
    });
}

/// Gives the event to one client, dropping its queue if it is gone.
pub(super) fn send_to(subscribers: &Subscribers, client: ClientId, event: Event) {
    subscribers.lock().retain(|subscriber| {
        subscriber.client != client || subscriber.queue.send(event.clone()).is_ok()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::serve::Subscriber;
    use dusk_protocol::thread::ThreadEvent;
    use tokio::sync::mpsc;

    fn full(client: ClientId, queue: mpsc::UnboundedSender<Event>) -> Subscriber {
        Subscriber {
            client,
            scope: crate::client::Scope::full(),
            queue,
        }
    }

    fn ended(thread_id: &str) -> Event {
        Event::Ended {
            thread_id: thread_id.to_owned(),
        }
    }

    #[test]
    fn every_attached_client_gets_each_event_in_order() {
        let subscribers = Subscribers::default();
        let (first, mut first_events) = mpsc::unbounded_channel();
        let (second, mut second_events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, first));
        subscribers.lock().push(full(2, second));

        publish(&subscribers, ended("a"));
        publish(&subscribers, ended("b"));

        for events in [&mut first_events, &mut second_events] {
            assert_eq!(events.try_recv().unwrap(), ended("a"));
            assert_eq!(events.try_recv().unwrap(), ended("b"));
        }
    }

    #[test]
    fn a_gone_client_is_dropped_and_the_others_keep_receiving() {
        let subscribers = Subscribers::default();
        let (gone, gone_events) = mpsc::unbounded_channel();
        let (stays, mut stays_events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, gone));
        subscribers.lock().push(full(2, stays));
        drop(gone_events);

        publish(&subscribers, ended("a"));

        assert_eq!(subscribers.lock().len(), 1);
        assert_eq!(stays_events.try_recv().unwrap(), ended("a"));
    }

    #[test]
    fn only_the_client_that_joined_hears_what_it_missed() {
        let subscribers = Subscribers::default();
        let (joined, mut joined_events) = mpsc::unbounded_channel();
        let (other, mut other_events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, joined));
        subscribers.lock().push(full(2, other));

        send_to(&subscribers, 1, ended("a"));

        assert_eq!(joined_events.try_recv().unwrap(), ended("a"));
        assert!(other_events.try_recv().is_err());
    }

    #[test]
    fn a_gone_client_targeted_by_itself_is_dropped() {
        let subscribers = Subscribers::default();
        let (gone, gone_events) = mpsc::unbounded_channel();
        let (stays, mut stays_events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, gone));
        subscribers.lock().push(full(2, stays));
        drop(gone_events);

        send_to(&subscribers, 1, ended("a"));

        assert_eq!(subscribers.lock().len(), 1);
        assert!(stays_events.try_recv().is_err());
    }

    #[test]
    fn a_client_bound_to_threads_gets_only_their_events() {
        use crate::client::{Area, Scope};
        let subscribers = Subscribers::default();
        let (all, mut all_events) = mpsc::unbounded_channel();
        let (one, mut one_events) = mpsc::unbounded_channel();
        let (other, mut other_events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, all));
        for (client, thread, queue) in [(2, "a", one), (3, "b", other)] {
            subscribers.lock().push(Subscriber {
                client,
                scope: Scope::threads([thread.to_owned()], &[Area::ThreadRead]),
                queue,
            });
        }
        let said = |thread_id: &str| Event::Thread {
            thread_id: thread_id.to_owned(),
            event: Box::new(ThreadEvent::Idle),
        };

        for event in [said("a"), ended("a"), said("b"), ended("b"), ended("c")] {
            publish(&subscribers, event);
        }

        let drain = |events: &mut mpsc::UnboundedReceiver<Event>| {
            std::iter::from_fn(|| events.try_recv().ok()).collect::<Vec<_>>()
        };
        assert_eq!(drain(&mut all_events).len(), 5);
        assert_eq!(drain(&mut one_events), [said("a"), ended("a")]);
        assert_eq!(drain(&mut other_events), [said("b"), ended("b")]);
        // A client kept out of a thread is not taken for a gone one.
        assert_eq!(subscribers.lock().len(), 3);
    }

    #[test]
    fn what_belongs_to_one_client_is_never_given_to_all() {
        let subscribers = Subscribers::default();
        let (queue, mut events) = mpsc::unbounded_channel();
        subscribers.lock().push(full(1, queue));

        publish(
            &subscribers,
            Event::Terminal {
                id: "t".to_owned(),
                data: b"secret".to_vec(),
            },
        );
        publish(
            &subscribers,
            Event::PluginNotice {
                plugin: "p".to_owned(),
                text: "note".to_owned(),
            },
        );

        assert!(events.try_recv().is_err());
    }
}
