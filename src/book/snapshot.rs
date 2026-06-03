use crate::types::{Order, OrderId, OrderType, Price, Quantity, Side};

use super::OrderBook;

// ---------------------------------------------------------------------------
// SnapshotError
// ---------------------------------------------------------------------------

/// Error returned by [`OrderBook::load`] when bytes are malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotError {
    /// The leading 8-byte magic does not match `b"MATCHER\x01"`.
    BadMagic,
    /// The 4-byte version field holds a value this build does not support.
    UnsupportedVersion(u32),
    /// The byte slice ends before all declared records could be parsed.
    Truncated,
    /// A record's side byte is not 1 (Buy) or 2 (Sell).
    InvalidSide(u8),
    /// A record's kind_tag byte is not 2 (Limit), 5 (PostOnly), or 6 (Iceberg).
    InvalidKind(u8),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::BadMagic => write!(f, "snapshot: bad magic bytes"),
            SnapshotError::UnsupportedVersion(v) => {
                write!(f, "snapshot: unsupported version {v}")
            }
            SnapshotError::Truncated => write!(f, "snapshot: payload truncated"),
            SnapshotError::InvalidSide(b) => write!(f, "snapshot: invalid side byte {b}"),
            SnapshotError::InvalidKind(b) => write!(f, "snapshot: invalid kind byte {b}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// ---------------------------------------------------------------------------
// Binary format constants
// ---------------------------------------------------------------------------

// Magic = b"MATCHER\x01" (8 bytes)
pub(super) const SNAP_MAGIC: &[u8; 8] = b"MATCHER\x01";
pub(super) const SNAP_VERSION: u32 = 1;

// Byte offsets within the fixed header.
pub(super) const HDR_MAGIC_END: usize = 8;
pub(super) const HDR_VERSION_END: usize = 12;
pub(super) const HDR_NORDERS_END: usize = 20;
pub(super) const HEADER_LEN: usize = 20;

// Per-record encoding constants (reuse codec side/kind tags).
pub(super) const SIDE_BUY: u8 = 1;
pub(super) const SIDE_SELL: u8 = 2;
pub(super) const KIND_LIMIT: u8 = 2;
pub(super) const KIND_POST_ONLY: u8 = 5;
pub(super) const KIND_ICEBERG: u8 = 6;

// Fixed bytes for a single record (before the optional iceberg_visible u64).
pub(super) const RECORD_BASE: usize = 49;

// ---------------------------------------------------------------------------
// impl OrderBook — snapshot / restore
// ---------------------------------------------------------------------------

impl OrderBook {
    /// Serialize the entire book to a self-describing binary blob.
    ///
    /// Round-tripping through [`OrderBook::load`] reproduces an identical book:
    /// same orders, same per-level FIFO time priority, same best_bid/best_ask.
    pub fn snapshot(&self) -> Vec<u8> {
        // Upper-bound capacity: header + worst-case all icebergs (57 bytes each).
        let n = self.orders.len();
        let mut buf = Vec::with_capacity(HEADER_LEN + n * (RECORD_BASE + 8));

        // Header
        buf.extend_from_slice(SNAP_MAGIC);
        buf.extend_from_slice(&SNAP_VERSION.to_be_bytes());
        buf.extend_from_slice(&(n as u64).to_be_bytes());

        // Emit orders in FIFO order per level.
        // Bids: ascending price (lowest first). Each level's orders list is
        // already in submission (FIFO) order.
        for level in self.bids.values() {
            for &id in &level.orders {
                if let Some(o) = self.orders.get(&id) {
                    Self::write_order_record(&mut buf, o);
                }
            }
        }
        // Asks: ascending price.
        for level in self.asks.values() {
            for &id in &level.orders {
                if let Some(o) = self.orders.get(&id) {
                    Self::write_order_record(&mut buf, o);
                }
            }
        }

        buf
    }

    pub(super) fn write_order_record(buf: &mut Vec<u8>, o: &Order) {
        // [0]    side
        buf.push(match o.side {
            Side::Buy => SIDE_BUY,
            Side::Sell => SIDE_SELL,
        });
        // [1]    kind_tag
        let kind_tag = match o.kind {
            OrderType::Limit => KIND_LIMIT,
            OrderType::PostOnly => KIND_POST_ONLY,
            OrderType::Iceberg { .. } => KIND_ICEBERG,
            // Market/IOC/FOK never rest; unreachable in practice.
            _ => unreachable!("non-resting order kind in snapshot"),
        };
        buf.push(kind_tag);
        // [2..8] reserved (6 zero bytes)
        buf.extend_from_slice(&[0u8; 6]);
        // [8..16]  id
        buf.extend_from_slice(&o.id.0.to_be_bytes());
        // [16..24] price
        buf.extend_from_slice(&o.price.0.to_be_bytes());
        // [24..32] quantity
        buf.extend_from_slice(&o.quantity.0.to_be_bytes());
        // [32..40] filled
        buf.extend_from_slice(&o.filled.0.to_be_bytes());
        // [40..48] hidden
        buf.extend_from_slice(&o.hidden.0.to_be_bytes());
        // [48]     iceberg_visible_set (0 or 1)
        if let OrderType::Iceberg { visible } = o.kind {
            buf.push(1u8);
            // +8 bytes: iceberg_visible
            buf.extend_from_slice(&visible.0.to_be_bytes());
        } else {
            buf.push(0u8);
        }
    }

    /// Reconstruct an [`OrderBook`] from bytes produced by [`OrderBook::snapshot`].
    ///
    /// Returns `Err` if the magic/version mismatches or the bytes are malformed.
    pub fn load(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let n_orders = Self::parse_snapshot_header(bytes)?;
        let mut book = OrderBook::new();
        let mut pos = HEADER_LEN;
        for _ in 0..n_orders {
            let (order, consumed) = Self::parse_order_record(&bytes[pos..], bytes.len() - pos)?;
            pos += consumed;
            // Bypass submit_events (which would match!) — directly rest the order.
            book.rest(order);
        }
        Ok(book)
    }

    /// Validate and parse the fixed 20-byte snapshot header; returns the order count.
    fn parse_snapshot_header(bytes: &[u8]) -> Result<usize, SnapshotError> {
        // Magic check: compare however many bytes we have against the magic
        // prefix. If they differ we know it's wrong magic (not just truncation).
        // Only report Truncated when the bytes so far do match the prefix.
        let magic_cmp_len = bytes.len().min(HDR_MAGIC_END);
        if bytes[..magic_cmp_len] != SNAP_MAGIC[..magic_cmp_len] {
            return Err(SnapshotError::BadMagic);
        }
        if bytes.len() < HEADER_LEN {
            return Err(SnapshotError::Truncated);
        }
        let version = u32::from_be_bytes(
            bytes[HDR_MAGIC_END..HDR_VERSION_END]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        if version != SNAP_VERSION {
            return Err(SnapshotError::UnsupportedVersion(version));
        }
        let n_orders = u64::from_be_bytes(
            bytes[HDR_VERSION_END..HDR_NORDERS_END]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        ) as usize;
        Ok(n_orders)
    }

    /// Parse one order record from `record_bytes` (a slice starting at the record).
    /// `remaining_len` is the number of bytes left in the full buffer (for bounds checks).
    /// Returns `(Order, bytes_consumed)`.
    ///
    /// Long but flat: 9 fixed-offset field reads followed by two enum decodings.
    /// Extracting sub-functions would require passing partially-parsed state across
    /// call boundaries, making the wire-format contract harder to verify by reading.
    #[allow(clippy::too_many_lines)]
    fn parse_order_record(
        record_bytes: &[u8],
        remaining_len: usize,
    ) -> Result<(Order, usize), SnapshotError> {
        if remaining_len < RECORD_BASE {
            return Err(SnapshotError::Truncated);
        }

        let side_byte = record_bytes[0];
        let kind_tag = record_bytes[1];
        // [2..8] reserved — skip
        let id = u64::from_be_bytes(
            record_bytes[8..16]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let price = u64::from_be_bytes(
            record_bytes[16..24]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let quantity = u64::from_be_bytes(
            record_bytes[24..32]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let filled = u64::from_be_bytes(
            record_bytes[32..40]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let hidden = u64::from_be_bytes(
            record_bytes[40..48]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let iceberg_visible_set = record_bytes[48];
        let mut consumed = RECORD_BASE;

        let side = match side_byte {
            SIDE_BUY => Side::Buy,
            SIDE_SELL => Side::Sell,
            other => return Err(SnapshotError::InvalidSide(other)),
        };

        let kind = match kind_tag {
            KIND_LIMIT => OrderType::Limit,
            KIND_POST_ONLY => OrderType::PostOnly,
            KIND_ICEBERG => {
                if iceberg_visible_set != 0 {
                    if remaining_len < RECORD_BASE + 8 {
                        return Err(SnapshotError::Truncated);
                    }
                    let vis = u64::from_be_bytes(
                        record_bytes[RECORD_BASE..RECORD_BASE + 8]
                            .try_into()
                            .map_err(|_| SnapshotError::Truncated)?,
                    );
                    consumed += 8;
                    OrderType::Iceberg {
                        visible: Quantity(vis),
                    }
                } else {
                    // iceberg_visible_set == 0: fall back to quantity as the visible slice.
                    OrderType::Iceberg {
                        visible: Quantity(quantity),
                    }
                }
            }
            other => return Err(SnapshotError::InvalidKind(other)),
        };

        let order = Order {
            id: OrderId(id),
            side,
            kind,
            price: Price(price),
            quantity: Quantity(quantity),
            filled: Quantity(filled),
            hidden: Quantity(hidden),
        };
        Ok((order, consumed))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Order, OrderId, Price, Quantity, Side};

    fn lim(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        qty: impl Into<Quantity>,
    ) -> Order {
        Order::limit(id, side, price, qty)
    }
    fn mkt(id: impl Into<OrderId>, side: Side, qty: impl Into<Quantity>) -> Order {
        Order::market(id, side, qty)
    }

    #[test]
    fn snapshot_empty_book_round_trips() {
        let b = OrderBook::new();
        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");
        assert_eq!(b2.len(), 0);
        assert_eq!(b2.best_bid(), None);
        assert_eq!(b2.best_ask(), None);
        b2.assert_invariants();
    }

    #[test]
    fn snapshot_limit_orders_round_trip_preserves_state() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        b.submit(lim(2, Side::Buy, 99, 5), 0);
        b.submit(lim(3, Side::Sell, 101, 8), 0);
        b.submit(lim(4, Side::Sell, 102, 3), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");

        assert_eq!(b2.len(), 4);
        assert_eq!(b2.best_bid(), Some(Price(100)));
        assert_eq!(b2.best_ask(), Some(Price(101)));
        assert_eq!(b2.level_qty(Side::Buy, 100u64), Quantity(10));
        assert_eq!(b2.level_qty(Side::Buy, 99u64), Quantity(5));
        assert_eq!(b2.level_qty(Side::Sell, 101u64), Quantity(8));
        assert_eq!(b2.level_qty(Side::Sell, 102u64), Quantity(3));
        b2.assert_invariants();
    }

    #[test]
    fn snapshot_preserves_fifo_priority() {
        // Two sell limits at the same price, older order first.
        let mut b = OrderBook::new();
        b.submit(lim(10, Side::Sell, 100, 5), 0); // older
        b.submit(lim(11, Side::Sell, 100, 5), 1); // newer
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // A buy market taker should trade with the older sell (id=10) first.
        let trades = b2.submit(mkt(99, Side::Buy, 5), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(
            trades[0].sell_id,
            OrderId(10),
            "FIFO violated: newer order traded first"
        );
        assert_eq!(trades[0].quantity, Quantity(5));
        // Older order fully consumed; newer still rests.
        assert_eq!(b2.len(), 1);
    }

    #[test]
    fn snapshot_iceberg_preserves_hidden_quantity() {
        // Iceberg: total=30, visible=10, hidden=20.
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // Drain the visible chunk — should refill from hidden.
        let t1 = b2.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t1.len(), 1);
        assert_eq!(t1[0].quantity, Quantity(10));
        assert_eq!(b2.best_ask(), Some(Price(100)), "iceberg did not refill");

        // Drain second visible chunk.
        let t2 = b2.submit(mkt(3, Side::Buy, 10), 2);
        assert_eq!(t2.len(), 1);
        assert_eq!(t2[0].quantity, Quantity(10));
        assert_eq!(
            b2.best_ask(),
            Some(Price(100)),
            "iceberg did not refill again"
        );

        // Drain final chunk — book should be empty.
        let t3 = b2.submit(mkt(4, Side::Buy, 10), 3);
        assert_eq!(t3.len(), 1);
        assert_eq!(t3[0].quantity, Quantity(10));
        assert_eq!(b2.best_ask(), None);
        assert_eq!(b2.len(), 0);
    }

    #[test]
    fn snapshot_partial_fills_preserve_filled() {
        // Submit a limit qty=10, partially fill 3, then snapshot.
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 10), 0);
        // Partial fill: taker buys 3.
        let trades = b.submit(mkt(2, Side::Buy, 3), 1);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, Quantity(3));
        // Resting sell order now has remaining=7.
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(7));
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // Remaining should still be 7 (i.e. quantity=7 with filled=0 after
        // partial fill was captured in the snapshot as quantity=7, filled=3
        // — but order_level_qty uses remaining() = quantity - filled).
        assert_eq!(b2.level_qty(Side::Sell, 100u64), Quantity(7));

        // Drain the remaining 7.
        let t = b2.submit(mkt(3, Side::Buy, 7), 2);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, Quantity(7));
        assert_eq!(b2.len(), 0);
    }

    #[test]
    fn load_rejects_bad_magic() {
        let result = OrderBook::load(b"BADMAGIC");
        assert!(
            matches!(result, Err(SnapshotError::BadMagic)),
            "expected BadMagic, got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn load_rejects_unsupported_version() {
        // Craft a header with valid magic but version=99.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MATCHER\x01"); // magic
        bytes.extend_from_slice(&99u32.to_be_bytes()); // version=99
        bytes.extend_from_slice(&0u64.to_be_bytes()); // n_orders=0
        let result = OrderBook::load(&bytes);
        assert!(
            matches!(result, Err(SnapshotError::UnsupportedVersion(99))),
            "expected UnsupportedVersion(99), got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn load_rejects_truncated_payload() {
        // Build a valid 1-order snapshot, then truncate the last few bytes.
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        let snap = b.snapshot();

        // Truncate: drop the last 5 bytes — cuts into the last record.
        let truncated = &snap[..snap.len() - 5];
        let result = OrderBook::load(truncated);
        assert!(
            matches!(result, Err(SnapshotError::Truncated)),
            "expected Truncated, got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn snapshot_then_load_passes_assert_invariants() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        b.submit(lim(2, Side::Buy, 100, 5), 0); // same level, FIFO
        b.submit(lim(3, Side::Sell, 101, 7), 0);
        b.submit(Order::post_only(4, Side::Sell, 102, 3), 0);
        b.submit(Order::iceberg(5, Side::Sell, 103, 50, 10), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        assert_eq!(b2.len(), b.len());
        assert_eq!(b2.best_bid(), b.best_bid());
        assert_eq!(b2.best_ask(), b.best_ask());
    }
}
