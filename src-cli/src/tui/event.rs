use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};
use tokio::sync::mpsc::{self, UnboundedReceiver};

pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
}

pub fn spawn_input_thread() -> UnboundedReceiver<InputEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(200)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(k)) => {
                        if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat {
                            if tx.send(InputEvent::Key(k)).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Event::Mouse(m)) => {
                        if tx.send(InputEvent::Mouse(m)).is_err() {
                            break;
                        }
                    }
                    Ok(Event::Resize(_, _)) => {
                        if tx.send(InputEvent::Resize).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });
    rx
}
