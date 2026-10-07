use iced_core::{Event, window::Id};

pub(super) fn batches(events: Vec<(Id, Event)>) -> impl Iterator<Item = (Id, Vec<Event>)> {
    let mut events = events.into_iter().peekable();

    std::iter::from_fn(move || {
        let (window, event) = events.next()?;
        let mut batch = vec![event];
        while events.peek().is_some_and(|(id, _)| *id == window) {
            batch.push(events.next().expect("peeked event").1);
        }
        Some((window, batch))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_core::keyboard::{self, Key, key::Named};

    fn release(key: Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
        })
    }

    fn press(key: Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn a_departing_windows_release_precedes_the_next_windows_press() {
        let main = Id::unique();
        let drawer = Id::unique();
        assert!(main < drawer);

        for key in [Named::Enter, Named::Space] {
            let events = vec![(drawer, release(key)), (main, press(key))];
            assert_eq!(
                batches(events).collect::<Vec<_>>(),
                vec![(drawer, vec![release(key)]), (main, vec![press(key)])],
            );
        }
    }

    #[test]
    fn returning_to_a_window_starts_another_batch() {
        let first = Id::unique();
        let second = Id::unique();
        let events = vec![
            (first, press(Named::Enter)),
            (first, release(Named::Enter)),
            (second, press(Named::Space)),
            (first, press(Named::Enter)),
        ];
        assert_eq!(
            batches(events).collect::<Vec<_>>(),
            vec![
                (first, vec![press(Named::Enter), release(Named::Enter)]),
                (second, vec![press(Named::Space)]),
                (first, vec![press(Named::Enter)]),
            ],
        );
        assert!(batches(Vec::new()).next().is_none());
    }
}
