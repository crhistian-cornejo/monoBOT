//! A call's ordered voice queue. Cancellation invalidates both a sentence
//! being played and the work still waiting behind it.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc,
    },
    thread,
};

use crate::{player, Voice};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Speaking {
        round: u64,
        member: String,
    },
    Finished {
        round: u64,
        member: String,
        error: Option<String>,
    },
    Drained {
        round: u64,
    },
}

/// Synthesis and playback used by a call's queue.
pub trait Backend: Send + 'static {
    fn synthesize(&self, text: &str, language: &str, speaker: i32) -> Result<Vec<u8>, String>;
    fn play(&self, audio: &[u8], stop: Arc<AtomicBool>) -> Result<(), String>;
}

impl Backend for Arc<Voice> {
    fn synthesize(&self, text: &str, language: &str, speaker: i32) -> Result<Vec<u8>, String> {
        self.speak(text, language, speaker)
            .map_err(|error| error.to_string())
    }

    fn play(&self, audio: &[u8], stop: Arc<AtomicBool>) -> Result<(), String> {
        player::play(audio, stop)
    }
}

enum Command {
    Say {
        round: u64,
        member: String,
        text: String,
        language: String,
        speaker: i32,
        stop: Arc<AtomicBool>,
    },
    Drain {
        round: u64,
        stop: Arc<AtomicBool>,
    },
}

/// A queue owned by one call. Its worker never blocks the UI thread.
pub struct Queue {
    commands: Sender<Command>,
    round: u64,
    stop: Arc<AtomicBool>,
}

impl Queue {
    pub fn new(voice: Arc<Voice>, event: impl Fn(Event) + Send + 'static) -> std::io::Result<Self> {
        Self::with_backend(voice, event)
    }

    pub fn with_backend(
        backend: impl Backend,
        event: impl Fn(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (commands, incoming) = mpsc::channel();
        thread::Builder::new()
            .name("group-call-voice".into())
            .spawn(move || {
                while let Ok(command) = incoming.recv() {
                    match command {
                        Command::Say {
                            round,
                            member,
                            text,
                            language,
                            speaker,
                            stop,
                        } => {
                            if stop.load(Ordering::Acquire) {
                                continue;
                            }
                            let result = backend.synthesize(&text, &language, speaker);
                            if stop.load(Ordering::Acquire) {
                                continue;
                            }
                            let result = result.and_then(|audio| {
                                event(Event::Speaking {
                                    round,
                                    member: member.clone(),
                                });
                                backend.play(&audio, stop.clone())
                            });
                            if !stop.load(Ordering::Acquire) {
                                event(Event::Finished {
                                    round,
                                    member,
                                    error: result.err(),
                                });
                            }
                        }
                        Command::Drain { round, stop } => {
                            if !stop.load(Ordering::Acquire) {
                                event(Event::Drained { round });
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            commands,
            round: 0,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Starts a new turn, interrupting any previous playback and queued work.
    pub fn begin_round(&mut self) -> u64 {
        self.stop.store(true, Ordering::Release);
        self.stop = Arc::new(AtomicBool::new(false));
        self.round = self.round.wrapping_add(1);
        self.round
    }

    pub fn say(
        &self,
        round: u64,
        member: &str,
        text: &str,
        language: &str,
        speaker: i32,
    ) -> Result<(), String> {
        if round != self.round || self.stop.load(Ordering::Acquire) {
            return Err("The call turn has ended".into());
        }
        self.commands
            .send(Command::Say {
                round,
                member: member.into(),
                text: text.into(),
                language: language.into(),
                speaker,
                stop: self.stop.clone(),
            })
            .map_err(|_| "The voice queue has stopped".into())
    }

    /// Reports Drained after every preceding sentence has played or failed.
    pub fn drain(&self, round: u64) -> Result<(), String> {
        if round != self.round || self.stop.load(Ordering::Acquire) {
            return Err("The call turn has ended".into());
        }
        self.commands
            .send(Command::Drain {
                round,
                stop: self.stop.clone(),
            })
            .map_err(|_| "The voice queue has stopped".into())
    }

    pub fn interrupt(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        self.interrupt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Fake {
        played: Sender<String>,
    }

    impl Backend for Fake {
        fn synthesize(&self, text: &str, _: &str, _: i32) -> Result<Vec<u8>, String> {
            Ok(text.as_bytes().to_vec())
        }

        fn play(&self, audio: &[u8], _: Arc<AtomicBool>) -> Result<(), String> {
            self.played
                .send(String::from_utf8_lossy(audio).into_owned())
                .unwrap();
            Ok(())
        }
    }

    #[test]
    fn sentences_play_in_order_before_drain() {
        let (played, sounds) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let mut queue = Queue::with_backend(Fake { played }, move |event| {
            events.send(event).unwrap();
        })
        .unwrap();
        let round = queue.begin_round();
        queue.say(round, "ana", "First.", "en", 0).unwrap();
        queue.say(round, "leo", "Second.", "en", 1).unwrap();
        queue.drain(round).unwrap();
        let wait = Duration::from_secs(2);
        assert_eq!(sounds.recv_timeout(wait).unwrap(), "First.");
        assert_eq!(sounds.recv_timeout(wait).unwrap(), "Second.");
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Speaking {
                round,
                member: "ana".into()
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Finished {
                round,
                member: "ana".into(),
                error: None
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Speaking {
                round,
                member: "leo".into()
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Finished {
                round,
                member: "leo".into(),
                error: None
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Drained { round }
        );
    }

    #[test]
    fn interruption_rejects_the_old_turn() {
        let (played, _) = mpsc::channel();
        let mut queue = Queue::with_backend(Fake { played }, |_| {}).unwrap();
        let old = queue.begin_round();
        queue.interrupt();
        assert!(queue.say(old, "ana", "Old", "en", 0).is_err());
        assert!(queue.drain(old).is_err());
        let next = queue.begin_round();
        assert_ne!(old, next);
        assert!(queue.drain(next).is_ok());
    }

    struct Blocking {
        started: Sender<()>,
        release: mpsc::Receiver<()>,
        played: Sender<String>,
        first: AtomicBool,
    }

    impl Backend for Blocking {
        fn synthesize(&self, text: &str, _: &str, _: i32) -> Result<Vec<u8>, String> {
            if self.first.swap(false, Ordering::AcqRel) {
                self.started.send(()).unwrap();
                self.release
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|error| error.to_string())?;
            }
            Ok(text.as_bytes().to_vec())
        }

        fn play(&self, audio: &[u8], _: Arc<AtomicBool>) -> Result<(), String> {
            self.played
                .send(String::from_utf8_lossy(audio).into_owned())
                .unwrap();
            Ok(())
        }
    }

    #[test]
    fn interruption_discards_inflight_and_queued_sentences() {
        let (started, starting) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (played, sounds) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let mut queue = Queue::with_backend(
            Blocking {
                started,
                release: released,
                played,
                first: AtomicBool::new(true),
            },
            move |event| {
                events.send(event).unwrap();
            },
        )
        .unwrap();
        let old = queue.begin_round();
        queue.say(old, "ana", "Old first.", "en", 0).unwrap();
        starting.recv_timeout(Duration::from_secs(2)).unwrap();
        queue.say(old, "ana", "Old second.", "en", 0).unwrap();
        queue.drain(old).unwrap();
        queue.interrupt();
        let next = queue.begin_round();
        queue.say(next, "leo", "New.", "en", 1).unwrap();
        queue.drain(next).unwrap();
        release.send(()).unwrap();
        let wait = Duration::from_secs(2);
        assert_eq!(sounds.recv_timeout(wait).unwrap(), "New.");
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Speaking {
                round: next,
                member: "leo".into()
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Finished {
                round: next,
                member: "leo".into(),
                error: None
            }
        );
        assert_eq!(
            received.recv_timeout(wait).unwrap(),
            Event::Drained { round: next }
        );
        assert!(sounds.try_recv().is_err());
        assert!(received.try_recv().is_err());
    }
}
