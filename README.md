# matcher

Order-matching engine in Rust — a `BTreeMap`-backed price-time book, no locks,
no I/O. The crate also ships a reference UDP-multicast daemon that wires the
book up to the network, but that's optional.

> **Status: experimental.** API and wire format may change between commits.

Supported order types: **Market**, **Limit**, **IOC**, **FOK**, **Post-Only**,
**Iceberg**. Time-in-force is GTC only. The book is single-threaded — wrap it
in your own actor / channel / lock if you need concurrency.

---

## Library usage

```rust
use matcher::{Order, OrderBook, Side};

let mut book = OrderBook::new();

// Resting sell @ 100, qty 5.
book.submit(Order::limit(1, Side::Sell, 100, 5), 0);

// Crossing buy @ 100, qty 3 — produces one trade.
let trades = book.submit(Order::limit(2, Side::Buy, 100, 3), 1);
assert_eq!(trades.len(), 1);
assert_eq!(trades[0].price, 100);
assert_eq!(trades[0].quantity, 3);
```

If callers need the full execution stream, use the event-returning API:

```rust
use matcher::{BookEvent, Order, OrderBook, Side};

let mut book = OrderBook::new();
let events = book.submit_events(Order::limit(1, Side::Buy, 100, 5), 0);

assert_eq!(events[0], BookEvent::Accepted { order_id: 1 });
assert_eq!(events[1], BookEvent::Rested { order_id: 1, remaining: 5 });
```

```toml
[dependencies]
matcher = { git = "https://github.com/gocronx/matcher" }
```

Pull in `matcher::book` only and `tokio` / `socket2` are unused at runtime.
Other deps: `ahash`, `smallvec`.

---

## Design

- **Single book, no internal sharding.** If you want N products, run N books
  (or N processes) — no router, no cross-product locks, one crash = one
  product.
- **No persistence.** Restart loads an empty book; the upstream order source
  is expected to replay or snapshot. Cuts the runtime in half.
- **Library first, transport optional.** UDP, gRPC, NATS, in-process — pick
  whatever fits. The bundled UDP daemon is an example, not the contract.

---

## Reference UDP daemon

A binary at `src/bin/matcher.rs` wires the book up to two UDP multicast
groups: orders in, trades out. Three async actors connected by mpsc channels;
the book is owned by the matcher actor exclusively, no locks anywhere.

![matcher architecture](images/matcher.jpg)

### Build & run

```sh
cargo build --release
./target/release/matcher --in 239.0.0.1:5000 --out 239.0.0.2:5001
```

`--in` / `--out` default to the values shown. Add `--iface 127.0.0.1` for
loopback-only testing (e.g. when a VPN is grabbing the multicast route).

### Wire protocol

64-byte big-endian packets. Byte 0 is the message type.

#### submit · type `1`

| offset     | field                                                          |
| ---------- | -------------------------------------------------------------- |
| `[1]`      | side (1=buy, 2=sell)                                           |
| `[2]`      | kind (1=market, 2=limit, 3=ioc, 4=fok, 5=post-only, 6=iceberg) |
| `[8..16]`  | order_id                                                       |
| `[16..24]` | price                                                          |
| `[24..32]` | total_quantity                                                 |
| `[32..40]` | iceberg_visible (0 unless iceberg)                             |

#### cancel · type `2`

| offset    | field    |
| --------- | -------- |
| `[8..16]` | order_id |

#### trade · type `3`

| offset     | field          |
| ---------- | -------------- |
| `[1]`      | aggressor side |
| `[8..16]`  | buy_id         |
| `[16..24]` | sell_id        |
| `[24..32]` | price          |
| `[32..40]` | quantity       |
| `[40..48]` | timestamp ns   |

`src/codec.rs` is the authoritative spec; `tests/smoke.rs` shows the same
path in Rust.

### Sending an order from Python

```python
import socket, struct

buf = bytearray(64)
buf[0] = 1                                # submit
buf[1] = 1                                # side: buy
buf[2] = 2                                # kind: limit
struct.pack_into(">QQQQ", buf, 8,
                 42,                      # order_id
                 100, 5, 0)               # price, total_qty, iceberg_visible

s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF,
             socket.inet_aton("127.0.0.1"))
s.sendto(bytes(buf), ("239.0.0.1", 5000))
```

---

## Testing

```sh
cargo test
```
