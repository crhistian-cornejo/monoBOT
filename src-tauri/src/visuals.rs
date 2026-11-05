//! The chat's live visuals. Each open visual keeps its interpreter, so a
//! slider or a tab changes that program's own state. Interpreters are not
//! `Send`, so they all live on one thread that the commands talk to.
use std::{
    collections::{HashMap, VecDeque},
    sync::{mpsc, Mutex, OnceLock},
};

use monocode_visual::{Node, VisualRuntime};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;

/// Open interpreters kept at once; the least recently used one closes first.
const MAX_OPEN: usize = 24;
/// What a dispatch to a closed visual answers, so the chat can reopen it.
const EXPIRED: &str = "visual-expired";

#[derive(Serialize)]
pub struct OpenedVisual {
    handle: u64,
    tree: Node,
}

enum Job {
    Open(String, oneshot::Sender<Result<OpenedVisual, String>>),
    Dispatch(u64, String, Value, oneshot::Sender<Result<Node, String>>),
    Close(u64),
}

#[derive(Default)]
struct Runtimes {
    next: u64,
    open: HashMap<u64, VisualRuntime>,
    order: VecDeque<u64>,
}

impl Runtimes {
    fn touch(&mut self, handle: u64) {
        self.order.retain(|open| *open != handle);
        self.order.push_back(handle);
    }

    fn open(&mut self, source: &str) -> Result<OpenedVisual, String> {
        let mut runtime = VisualRuntime::new(source)?;
        let tree = runtime.render()?;
        self.next += 1;
        let handle = self.next;
        self.open.insert(handle, runtime);
        self.touch(handle);
        while self.order.len() > MAX_OPEN {
            if let Some(oldest) = self.order.pop_front() {
                self.open.remove(&oldest);
            }
        }
        Ok(OpenedVisual { handle, tree })
    }

    fn dispatch(&mut self, handle: u64, handler: &str, value: Value) -> Result<Node, String> {
        let runtime = self.open.get_mut(&handle).ok_or(EXPIRED)?;
        let tree = runtime.dispatch(handler, value);
        self.touch(handle);
        tree
    }

    fn close(&mut self, handle: u64) {
        self.open.remove(&handle);
        self.order.retain(|open| *open != handle);
    }
}

fn worker() -> Result<mpsc::Sender<Job>, String> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let sender = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("visuals".into())
            .spawn(move || {
                let mut runtimes = Runtimes::default();
                for job in rx {
                    match job {
                        Job::Open(source, reply) => {
                            let _ = reply.send(runtimes.open(&source));
                        }
                        Job::Dispatch(handle, handler, value, reply) => {
                            let _ = reply.send(runtimes.dispatch(handle, &handler, value));
                        }
                        Job::Close(handle) => runtimes.close(handle),
                    }
                }
            })
            .expect("visual thread");
        Mutex::new(tx)
    });
    sender
        .lock()
        .map(|sender| sender.clone())
        .map_err(|_| "Visuals are unavailable".into())
}

#[tauri::command]
pub async fn visual_open(source: String) -> Result<OpenedVisual, String> {
    let (tx, rx) = oneshot::channel();
    worker()?
        .send(Job::Open(source, tx))
        .map_err(|_| "Visuals are unavailable")?;
    rx.await.map_err(|_| "Visuals are unavailable")?
}

#[tauri::command]
pub async fn visual_dispatch(handle: u64, handler: String, value: Value) -> Result<Node, String> {
    let (tx, rx) = oneshot::channel();
    worker()?
        .send(Job::Dispatch(handle, handler, value, tx))
        .map_err(|_| "Visuals are unavailable")?;
    rx.await.map_err(|_| "Visuals are unavailable")?
}

#[tauri::command]
pub fn visual_close(handle: u64) {
    if let Ok(worker) = worker() {
        let _ = worker.send(Job::Close(handle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const COUNTER: &str = "const s = visual.state({n: 0});\nreturn () => visual.button('n=' + s.n, () => { s.n += 1; });";

    fn handler(node: &Node) -> String {
        match node {
            Node::Button { handler, .. } => handler.clone(),
            _ => panic!("expected a button"),
        }
    }

    #[test]
    fn keeps_state_per_visual_and_closes_the_oldest() {
        let mut runtimes = Runtimes::default();
        let first = runtimes.open(COUNTER).unwrap();
        let tree = runtimes
            .dispatch(first.handle, &handler(&first.tree), json!(null))
            .unwrap();
        assert!(matches!(&tree, Node::Button { label, .. } if label == "n=1"));
        for _ in 0..MAX_OPEN {
            runtimes.open(COUNTER).unwrap();
        }
        assert_eq!(
            runtimes
                .dispatch(first.handle, &handler(&tree), json!(null))
                .unwrap_err(),
            EXPIRED
        );
        assert_eq!(runtimes.open.len(), MAX_OPEN);
    }
}
