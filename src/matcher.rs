//! Matching actor: owns the OrderBook, consumes Inbound, emits Trade.
//!
//! Single-threaded by construction — the actor task is the only owner of the
//! book, so no locks anywhere.

use crate::book::OrderBook;
use crate::codec::Inbound;
use crate::types::{Timestamp, Trade};
use tokio::sync::mpsc;

pub async fn run(
    mut inbound: mpsc::Receiver<Inbound>,
    trades: mpsc::Sender<Trade>,
    now_ns: impl Fn() -> Timestamp,
) {
    let mut book = OrderBook::new();
    while let Some(msg) = inbound.recv().await {
        match msg {
            Inbound::Submit(order) => {
                for trade in book.submit(order, now_ns()) {
                    if trades.send(trade).await.is_err() {
                        return;
                    }
                }
            }
            Inbound::Cancel(id) => {
                book.cancel(id);
            }
        }
    }
}
