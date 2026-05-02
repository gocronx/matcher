//! Wire format: fixed 64-byte big-endian packets, one struct per message
//! type. UDP already provides a 16-bit checksum so we don't add another.
//!
//! Layout (offsets in bytes):
//!
//!   submit  [0]=1 [1]=side [2]=kind [8..16]=order_id [16..24]=price
//!           [24..32]=total_quantity [32..40]=iceberg_visible
//!   cancel  [0]=2 [8..16]=order_id
//!   trade   [0]=3 [1]=aggressor [8..16]=buy_id [16..24]=sell_id
//!           [24..32]=price [32..40]=quantity [40..48]=ts

use crate::types::{Order, OrderId, OrderType, Quantity, Side, Trade};

pub const PACKET_SIZE: usize = 64;

const MSG_SUBMIT: u8 = 1;
const MSG_CANCEL: u8 = 2;
const MSG_TRADE: u8 = 3;

const SIDE_BUY: u8 = 1;
const SIDE_SELL: u8 = 2;

const KIND_MARKET: u8 = 1;
const KIND_LIMIT: u8 = 2;
const KIND_IOC: u8 = 3;
const KIND_FOK: u8 = 4;
const KIND_POST_ONLY: u8 = 5;
const KIND_ICEBERG: u8 = 6;

#[derive(Debug)]
#[allow(dead_code)] // payloads are used via Debug for diagnostics
pub enum DecodeError {
    WrongSize,
    UnknownMsgType(u8),
    UnknownSide(u8),
    UnknownKind(u8),
}

#[derive(Debug, Clone)]
pub enum Inbound {
    Submit(Order),
    Cancel(OrderId),
}

pub fn encode_submit(o: &Order) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MSG_SUBMIT;
    buf[1] = encode_side(o.side);
    let (kind, visible) = encode_kind(o.kind);
    buf[2] = kind;
    buf[8..16].copy_from_slice(&o.id.to_be_bytes());
    buf[16..24].copy_from_slice(&o.price.to_be_bytes());
    // Wire field is the user-facing total quantity. For icebergs the engine
    // splits this into a visible chunk + hidden pool; recover total here.
    let total = o.quantity.saturating_add(o.hidden);
    buf[24..32].copy_from_slice(&total.to_be_bytes());
    buf[32..40].copy_from_slice(&visible.to_be_bytes());
    buf
}

pub fn encode_cancel(id: OrderId) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MSG_CANCEL;
    buf[8..16].copy_from_slice(&id.to_be_bytes());
    buf
}

pub fn encode_trade(t: &Trade) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MSG_TRADE;
    buf[1] = encode_side(t.aggressor);
    buf[8..16].copy_from_slice(&t.buy_id.to_be_bytes());
    buf[16..24].copy_from_slice(&t.sell_id.to_be_bytes());
    buf[24..32].copy_from_slice(&t.price.to_be_bytes());
    buf[32..40].copy_from_slice(&t.quantity.to_be_bytes());
    buf[40..48].copy_from_slice(&t.ts.to_be_bytes());
    buf
}

pub fn decode_inbound(buf: &[u8]) -> Result<Inbound, DecodeError> {
    if buf.len() != PACKET_SIZE {
        return Err(DecodeError::WrongSize);
    }
    match buf[0] {
        MSG_SUBMIT => Ok(Inbound::Submit(decode_order(buf)?)),
        MSG_CANCEL => Ok(Inbound::Cancel(read_u64(buf, 8))),
        other => Err(DecodeError::UnknownMsgType(other)),
    }
}

pub fn decode_trade(buf: &[u8]) -> Result<Trade, DecodeError> {
    if buf.len() != PACKET_SIZE {
        return Err(DecodeError::WrongSize);
    }
    if buf[0] != MSG_TRADE {
        return Err(DecodeError::UnknownMsgType(buf[0]));
    }
    Ok(Trade {
        aggressor: decode_side(buf[1])?,
        buy_id: read_u64(buf, 8),
        sell_id: read_u64(buf, 16),
        price: read_u64(buf, 24),
        quantity: read_u64(buf, 32),
        ts: read_u64(buf, 40),
    })
}

fn decode_order(buf: &[u8]) -> Result<Order, DecodeError> {
    let side = decode_side(buf[1])?;
    let total = read_u64(buf, 24);
    let visible = read_u64(buf, 32);
    let kind = decode_kind(buf[2], visible)?;
    // Engine state holds the *visible chunk* in `quantity` and the rest of
    // the user's order in `hidden`; the matcher refills `quantity` from
    // `hidden` whenever a visible chunk is consumed.
    let (quantity, hidden) = match kind {
        OrderType::Iceberg { visible: v } => {
            let chunk = v.min(total);
            (chunk, total.saturating_sub(chunk))
        }
        _ => (total, 0),
    };
    Ok(Order {
        id: read_u64(buf, 8),
        side,
        kind,
        price: read_u64(buf, 16),
        quantity,
        filled: 0,
        hidden,
    })
}

fn encode_side(s: Side) -> u8 {
    match s {
        Side::Buy => SIDE_BUY,
        Side::Sell => SIDE_SELL,
    }
}

fn decode_side(b: u8) -> Result<Side, DecodeError> {
    match b {
        SIDE_BUY => Ok(Side::Buy),
        SIDE_SELL => Ok(Side::Sell),
        other => Err(DecodeError::UnknownSide(other)),
    }
}

fn encode_kind(k: OrderType) -> (u8, Quantity) {
    match k {
        OrderType::Market => (KIND_MARKET, 0),
        OrderType::Limit => (KIND_LIMIT, 0),
        OrderType::Ioc => (KIND_IOC, 0),
        OrderType::Fok => (KIND_FOK, 0),
        OrderType::PostOnly => (KIND_POST_ONLY, 0),
        OrderType::Iceberg { visible } => (KIND_ICEBERG, visible),
    }
}

fn decode_kind(b: u8, visible: Quantity) -> Result<OrderType, DecodeError> {
    Ok(match b {
        KIND_MARKET => OrderType::Market,
        KIND_LIMIT => OrderType::Limit,
        KIND_IOC => OrderType::Ioc,
        KIND_FOK => OrderType::Fok,
        KIND_POST_ONLY => OrderType::PostOnly,
        KIND_ICEBERG => OrderType::Iceberg { visible },
        other => return Err(DecodeError::UnknownKind(other)),
    })
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_be_bytes(buf[off..off + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_order() -> Order {
        Order {
            id: 42,
            side: Side::Buy,
            kind: OrderType::Limit,
            price: 12_345,
            quantity: 100,
            filled: 0,
            hidden: 0,
        }
    }

    #[test]
    fn submit_round_trip() {
        let buf = encode_submit(&sample_order());
        match decode_inbound(&buf).unwrap() {
            Inbound::Submit(o) => {
                assert_eq!(o.id, 42);
                assert_eq!(o.price, 12_345);
                assert_eq!(o.quantity, 100);
                assert!(matches!(o.kind, OrderType::Limit));
                assert_eq!(o.side, Side::Buy);
            }
            _ => panic!("expected Submit"),
        }
    }

    #[test]
    fn cancel_round_trip() {
        let buf = encode_cancel(7);
        match decode_inbound(&buf).unwrap() {
            Inbound::Cancel(id) => assert_eq!(id, 7),
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn iceberg_carries_visible_and_hidden() {
        // Sender side: total = quantity + hidden = 100, visible = 30.
        let o = Order {
            id: 1,
            side: Side::Sell,
            kind: OrderType::Iceberg { visible: 30 },
            price: 50,
            quantity: 30,
            filled: 0,
            hidden: 70,
        };
        let buf = encode_submit(&o);
        let Inbound::Submit(decoded) = decode_inbound(&buf).unwrap() else {
            panic!("submit");
        };
        assert!(matches!(decoded.kind, OrderType::Iceberg { visible: 30 }));
        assert_eq!(decoded.quantity, 30);
        assert_eq!(decoded.hidden, 70);
    }

    #[test]
    fn trade_round_trip() {
        let trade = Trade {
            buy_id: 10,
            sell_id: 11,
            price: 500,
            quantity: 25,
            ts: 1_000,
            aggressor: Side::Sell,
        };
        let buf = encode_trade(&trade);
        let back = decode_trade(&buf).unwrap();
        assert_eq!(back.buy_id, 10);
        assert_eq!(back.sell_id, 11);
        assert_eq!(back.aggressor, Side::Sell);
    }

    #[test]
    fn rejects_unknown_msg_type() {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0] = 99;
        assert!(matches!(decode_inbound(&buf), Err(DecodeError::UnknownMsgType(99))));
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(matches!(decode_inbound(&[0u8; 16]), Err(DecodeError::WrongSize)));
    }
}
