# Matcher API 文档 / API Documentation

[中文文档](#中文文档) | [English Documentation](#english-documentation)

---

## 中文文档

### 核心 API

#### Engine

主引擎，负责管理整个撮合系统。

##### 创建引擎

```rust
use matcher::{Config, Engine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let mut engine = Engine::new(config).await?;
    Ok(())
}
```

##### 方法

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `async fn new(config: Config) -> Result<Self>` | 创建新引擎实例 |
| `start` | `async fn start(&mut self) -> Result<()>` | 启动引擎 |
| `matching_engine` | `fn matching_engine(&self) -> Arc<MatchingEngine>` | 获取撮合引擎引用 |

---

#### MatchingEngine

撮合引擎，处理订单提交和撮合逻辑。

##### 订单操作

###### 提交订单

```rust
let order = Order::limit(
    "BTC-USD".to_string(),
    Side::Buy,
    50000,
    100,
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(order).await?;
```

**返回值**: `Vec<MatchResult>` - 成交结果列表

###### 取消订单

```rust
use uuid::Uuid;

let order_id: Uuid = /* ... */;
matching_engine.cancel_order(&order_id).await?;
```

##### 查询操作

###### 获取最优买卖价

```rust
let (best_bid, best_ask) = matching_engine.get_best_prices(&product_id)?;

// best_bid: Option<Price>
// best_ask: Option<Price>
```

###### 获取价差

```rust
let spread = matching_engine.get_spread(&product_id)?;

// spread: Option<Price>
// 返回买卖价差，如果没有买价或卖价则返回 None
```

###### 获取订单簿快照

```rust
let depth = 10;  // 获取前 10 档
let (bids, asks) = matching_engine.get_order_book_snapshot(&product_id, depth)?;

// bids: Vec<BookLevel>
// asks: Vec<BookLevel>
```

###### 获取市场深度

```rust
let (bid_depth, ask_depth) = matching_engine.get_depth(&product_id)?;

// bid_depth: usize - 买单总数
// ask_depth: usize - 卖单总数
```

###### 获取引擎统计

```rust
let stats = matching_engine.get_stats();

println!("订单总数: {}", stats.orders_received);
println!("撮合订单: {}", stats.orders_matched);
println!("成交笔数: {}", stats.trades_executed);
println!("平均延迟: {}ns", stats.avg_match_latency_ns);
println!("运行时间: {}s", stats.uptime_seconds);
```

---

#### Order

订单类型，表示一个交易订单。

##### 构造方法

###### 市价单

```rust
let order = Order::market(
    product_id: String,    // 交易对 ID，如 "BTC-USD"
    side: Side,            // 买卖方向: Side::Buy 或 Side::Sell
    quantity: u64,         // 数量
    submit_time: u64,      // 提交时间（纳秒）
);
```

**特点**:
- 立即以最优价格成交
- 不保证成交价格
- 可能部分成交

###### 限价单

```rust
let order = Order::limit(
    product_id: String,    // 交易对 ID
    side: Side,            // 买卖方向
    price: u64,            // 限价（单位：分，如 50000 = $500.00）
    quantity: u64,         // 数量
    submit_time: u64,      // 提交时间
);
```

**特点**:
- 只在指定价格或更优价格成交
- 未成交部分保留在订单簿中
- 可能部分成交

###### Post-Only 订单

```rust
let order = Order::post_only(
    product_id: String,    // 交易对 ID
    side: Side,            // 买卖方向
    price: u64,            // 限价
    quantity: u64,         // 数量
    submit_time: u64,      // 提交时间
);
```

**特点**:
- 只做 Maker，不做 Taker
- 如果会立即成交，订单被拒绝
- 保证获得 Maker 手续费优惠

###### 冰山单

```rust
let order = Order::iceberg(
    product_id: String,       // 交易对 ID
    side: Side,               // 买卖方向
    price: u64,               // 限价
    total_quantity: u64,      // 总数量
    visible_quantity: u64,    // 显示数量
    submit_time: u64,         // 提交时间
);
```

**特点**:
- 隐藏真实订单量
- 订单簿中只显示部分数量
- 适合大额订单，避免市场冲击

##### 订单字段

```rust
pub struct Order {
    pub id: OrderId,                    // 订单 ID (UUID)
    pub product_id: ProductId,          // 交易对 ID
    pub side: Side,                     // 买卖方向
    pub order_type: OrderType,          // 订单类型
    pub price: Option<Price>,           // 价格（市价单为 None）
    pub quantity: Quantity,             // 数量
    pub filled_quantity: Quantity,      // 已成交数量
    pub submit_time: Timestamp,         // 提交时间
    pub metadata: OrderMetadata,        // 元数据
}
```

##### 订单方法

```rust
// 获取剩余数量
let remaining = order.remaining_quantity();

// 检查是否完全成交
if order.is_filled() {
    println!("订单已完全成交");
}

// 检查订单是否仍然有效
if order.is_active(current_time) {
    println!("订单仍然有效");
}

// 检查是否可以与另一个订单撮合
if order.can_match(&other_order, current_time) {
    println!("可以撮合");
}
```

---

#### 类型定义

##### Side - 买卖方向

```rust
pub enum Side {
    Buy,   // 买入
    Sell,  // 卖出
}
```

##### OrderType - 订单类型

```rust
pub enum OrderType {
    Market,                              // 市价单
    Limit,                               // 限价单
    IOC,                                 // 立即成交或取消
    FOK,                                 // 全部成交或全部取消
    PostOnly,                            // 只做 Maker
    Iceberg { visible_quantity: u64 },   // 冰山单
    StopLimit { stop_price: u64 },       // 止损限价单
    StopMarket { stop_price: u64 },      // 止损市价单
    GTD { expire_time: u64 },            // 指定日期过期
    Day,                                 // 当日有效
}
```

##### MatchResult - 成交结果

```rust
pub struct MatchResult {
    pub maker_order_id: OrderId,      // Maker 订单 ID
    pub taker_order_id: OrderId,      // Taker 订单 ID
    pub price: Price,                 // 成交价格
    pub quantity: Quantity,           // 成交数量
    pub timestamp: Timestamp,         // 成交时间
    pub match_latency_ns: u64,        // 撮合延迟（纳秒）
}
```

##### BookLevel - 订单簿档位

```rust
pub struct BookLevel {
    pub price: Price,        // 价格
    pub quantity: Quantity,  // 数量
}
```

##### EngineStats - 引擎统计

```rust
pub struct EngineStats {
    pub orders_received: u64,       // 接收订单总数
    pub orders_matched: u64,        // 撮合订单总数
    pub trades_executed: u64,       // 成交笔数
    pub avg_match_latency_ns: u64,  // 平均撮合延迟
    pub uptime_seconds: u64,        // 运行时间
}
```

---

### 配置 API

#### Config

引擎配置结构。

```rust
use matcher::Config;

let mut config = Config::default();

// 引擎配置
config.engine.product_ids = vec!["BTC-USD".to_string(), "ETH-USD".to_string()];
config.engine.max_orders_per_product = 1000000;

// 网络配置
config.network.listen_port = 8080;
config.network.multicast_addr = "239.0.0.1:5000".to_string();

// 性能配置
config.performance.batch_size = 100;
config.performance.worker_threads = 4;
config.performance.use_fast_hash = true;
config.performance.use_object_pool = true;
config.performance.use_smallvec = true;
```

##### 配置字段

```rust
pub struct Config {
    pub engine: EngineConfig,
    pub network: NetworkConfig,
    pub performance: PerformanceConfig,
}

pub struct EngineConfig {
    pub product_ids: Vec<String>,
    pub max_orders_per_product: usize,
}

pub struct NetworkConfig {
    pub listen_port: u16,
    pub multicast_addr: String,
    pub broadcast_addr: String,
}

pub struct PerformanceConfig {
    pub batch_size: usize,
    pub worker_threads: usize,
    pub use_fast_hash: bool,
    pub use_object_pool: bool,
    pub use_smallvec: bool,
}
```

---

### 工具函数

#### 时间戳

```rust
use matcher::utils::current_timestamp_ns;

// 获取当前时间戳（纳秒）
let timestamp = current_timestamp_ns();
```

#### 高精度计时器

```rust
use matcher::utils::HighResTimer;

let timer = HighResTimer::new();
// ... 执行操作 ...
let elapsed_ns = timer.elapsed_ns();
println!("耗时: {}ns", elapsed_ns);
```

---

### 错误处理

#### MatcherError

```rust
pub enum MatcherError {
    Config(String),                              // 配置错误
    Network(std::io::Error),                     // 网络错误
    OrderNotFound { order_id: OrderId },         // 订单未找到
    ProductNotSupported { product_id: String },  // 不支持的交易对
    Engine(String),                              // 引擎错误
}
```

#### 错误处理示例

```rust
use matcher::MatcherError;

match matching_engine.submit_order(order).await {
    Ok(matches) => {
        println!("成交: {} 笔", matches.len());
    }
    Err(MatcherError::ProductNotSupported { product_id }) => {
        eprintln!("不支持的交易对: {}", product_id);
    }
    Err(e) => {
        eprintln!("错误: {}", e);
    }
}
```

---

### 完整示例

#### 基础交易流程

```rust
use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建并启动引擎
    let config = Config::default();
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    let matching_engine = engine.matching_engine();
    
    // 2. 提交卖单（Maker）
    let sell_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Sell,
        50100,  // $501.00
        100,
        current_timestamp_ns(),
    );
    matching_engine.submit_order(sell_order).await?;
    
    // 3. 提交买单（Taker）
    let buy_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,  // 匹配卖单价格
        50,
        current_timestamp_ns(),
    );
    let matches = matching_engine.submit_order(buy_order).await?;
    
    // 4. 处理成交结果
    for m in matches {
        println!("成交: {} @ ${:.2}", m.quantity, m.price as f64 / 100.0);
    }
    
    // 5. 查询订单簿
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    println!("最优买价: {:?}, 最优卖价: {:?}", best_bid, best_ask);
    
    Ok(())
}
```

---

## English Documentation

### Core API

#### Engine

Main engine responsible for managing the entire matching system.

##### Creating an Engine

```rust
use matcher::{Config, Engine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let mut engine = Engine::new(config).await?;
    Ok(())
}
```

##### Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `async fn new(config: Config) -> Result<Self>` | Create new engine instance |
| `start` | `async fn start(&mut self) -> Result<()>` | Start the engine |
| `matching_engine` | `fn matching_engine(&self) -> Arc<MatchingEngine>` | Get matching engine reference |

---

#### MatchingEngine

Matching engine that handles order submission and matching logic.

##### Order Operations

###### Submit Order

```rust
let order = Order::limit(
    "BTC-USD".to_string(),
    Side::Buy,
    50000,
    100,
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(order).await?;
```

**Returns**: `Vec<MatchResult>` - List of match results

###### Cancel Order

```rust
use uuid::Uuid;

let order_id: Uuid = /* ... */;
matching_engine.cancel_order(&order_id).await?;
```

##### Query Operations

###### Get Best Prices

```rust
let (best_bid, best_ask) = matching_engine.get_best_prices(&product_id)?;

// best_bid: Option<Price>
// best_ask: Option<Price>
```

###### Get Spread

```rust
let spread = matching_engine.get_spread(&product_id)?;

// spread: Option<Price>
// Returns bid-ask spread, None if no bid or ask
```

###### Get Order Book Snapshot

```rust
let depth = 10;  // Get top 10 levels
let (bids, asks) = matching_engine.get_order_book_snapshot(&product_id, depth)?;

// bids: Vec<BookLevel>
// asks: Vec<BookLevel>
```

###### Get Market Depth

```rust
let (bid_depth, ask_depth) = matching_engine.get_depth(&product_id)?;

// bid_depth: usize - Total number of buy orders
// ask_depth: usize - Total number of sell orders
```

###### Get Engine Statistics

```rust
let stats = matching_engine.get_stats();

println!("Orders received: {}", stats.orders_received);
println!("Orders matched: {}", stats.orders_matched);
println!("Trades executed: {}", stats.trades_executed);
println!("Average latency: {}ns", stats.avg_match_latency_ns);
println!("Uptime: {}s", stats.uptime_seconds);
```

---

#### Order

Order type representing a trading order.

##### Constructor Methods

###### Market Order

```rust
let order = Order::market(
    product_id: String,    // Product ID, e.g., "BTC-USD"
    side: Side,            // Side: Side::Buy or Side::Sell
    quantity: u64,         // Quantity
    submit_time: u64,      // Submit time (nanoseconds)
);
```

**Characteristics**:
- Executes immediately at best available price
- No price guarantee
- May partially fill

###### Limit Order

```rust
let order = Order::limit(
    product_id: String,    // Product ID
    side: Side,            // Side
    price: u64,            // Limit price (in cents, e.g., 50000 = $500.00)
    quantity: u64,         // Quantity
    submit_time: u64,      // Submit time
);
```

**Characteristics**:
- Only executes at specified price or better
- Unfilled portion remains in order book
- May partially fill

###### Post-Only Order

```rust
let order = Order::post_only(
    product_id: String,    // Product ID
    side: Side,            // Side
    price: u64,            // Limit price
    quantity: u64,         // Quantity
    submit_time: u64,      // Submit time
);
```

**Characteristics**:
- Maker only, not taker
- Rejected if would immediately match
- Guarantees maker fee rebate

###### Iceberg Order

```rust
let order = Order::iceberg(
    product_id: String,       // Product ID
    side: Side,               // Side
    price: u64,               // Limit price
    total_quantity: u64,      // Total quantity
    visible_quantity: u64,    // Visible quantity
    submit_time: u64,         // Submit time
);
```

**Characteristics**:
- Hides true order size
- Only partial quantity visible in order book
- Suitable for large orders to avoid market impact

##### Order Fields

```rust
pub struct Order {
    pub id: OrderId,                    // Order ID (UUID)
    pub product_id: ProductId,          // Product ID
    pub side: Side,                     // Side
    pub order_type: OrderType,          // Order type
    pub price: Option<Price>,           // Price (None for market orders)
    pub quantity: Quantity,             // Quantity
    pub filled_quantity: Quantity,      // Filled quantity
    pub submit_time: Timestamp,         // Submit time
    pub metadata: OrderMetadata,        // Metadata
}
```

##### Order Methods

```rust
// Get remaining quantity
let remaining = order.remaining_quantity();

// Check if fully filled
if order.is_filled() {
    println!("Order is fully filled");
}

// Check if order is still active
if order.is_active(current_time) {
    println!("Order is still active");
}

// Check if can match with another order
if order.can_match(&other_order, current_time) {
    println!("Can match");
}
```

---

#### Type Definitions

##### Side

```rust
pub enum Side {
    Buy,   // Buy
    Sell,  // Sell
}
```

##### OrderType

```rust
pub enum OrderType {
    Market,                              // Market order
    Limit,                               // Limit order
    IOC,                                 // Immediate-or-Cancel
    FOK,                                 // Fill-or-Kill
    PostOnly,                            // Maker only
    Iceberg { visible_quantity: u64 },   // Iceberg order
    StopLimit { stop_price: u64 },       // Stop limit order
    StopMarket { stop_price: u64 },      // Stop market order
    GTD { expire_time: u64 },            // Good-Till-Date
    Day,                                 // Day order
}
```

##### MatchResult

```rust
pub struct MatchResult {
    pub maker_order_id: OrderId,      // Maker order ID
    pub taker_order_id: OrderId,      // Taker order ID
    pub price: Price,                 // Match price
    pub quantity: Quantity,           // Match quantity
    pub timestamp: Timestamp,         // Match timestamp
    pub match_latency_ns: u64,        // Match latency (nanoseconds)
}
```

##### BookLevel

```rust
pub struct BookLevel {
    pub price: Price,        // Price
    pub quantity: Quantity,  // Quantity
}
```

##### EngineStats

```rust
pub struct EngineStats {
    pub orders_received: u64,       // Total orders received
    pub orders_matched: u64,        // Total orders matched
    pub trades_executed: u64,       // Total trades executed
    pub avg_match_latency_ns: u64,  // Average match latency
    pub uptime_seconds: u64,        // Uptime in seconds
}
```

---

### Configuration API

#### Config

Engine configuration structure.

```rust
use matcher::Config;

let mut config = Config::default();

// Engine configuration
config.engine.product_ids = vec!["BTC-USD".to_string(), "ETH-USD".to_string()];
config.engine.max_orders_per_product = 1000000;

// Network configuration
config.network.listen_port = 8080;
config.network.multicast_addr = "239.0.0.1:5000".to_string();

// Performance configuration
config.performance.batch_size = 100;
config.performance.worker_threads = 4;
config.performance.use_fast_hash = true;
config.performance.use_object_pool = true;
config.performance.use_smallvec = true;
```

---

### Utility Functions

#### Timestamp

```rust
use matcher::utils::current_timestamp_ns;

// Get current timestamp (nanoseconds)
let timestamp = current_timestamp_ns();
```

#### High-Resolution Timer

```rust
use matcher::utils::HighResTimer;

let timer = HighResTimer::new();
// ... perform operations ...
let elapsed_ns = timer.elapsed_ns();
println!("Elapsed: {}ns", elapsed_ns);
```

---

### Error Handling

#### MatcherError

```rust
pub enum MatcherError {
    Config(String),                              // Configuration error
    Network(std::io::Error),                     // Network error
    OrderNotFound { order_id: OrderId },         // Order not found
    ProductNotSupported { product_id: String },  // Product not supported
    Engine(String),                              // Engine error
}
```

#### Error Handling Example

```rust
use matcher::MatcherError;

match matching_engine.submit_order(order).await {
    Ok(matches) => {
        println!("Matches: {}", matches.len());
    }
    Err(MatcherError::ProductNotSupported { product_id }) => {
        eprintln!("Product not supported: {}", product_id);
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

---

### Complete Example

#### Basic Trading Flow

```rust
use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create and start engine
    let config = Config::default();
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    let matching_engine = engine.matching_engine();
    
    // 2. Submit sell order (Maker)
    let sell_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Sell,
        50100,  // $501.00
        100,
        current_timestamp_ns(),
    );
    matching_engine.submit_order(sell_order).await?;
    
    // 3. Submit buy order (Taker)
    let buy_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,  // Match sell order price
        50,
        current_timestamp_ns(),
    );
    let matches = matching_engine.submit_order(buy_order).await?;
    
    // 4. Process match results
    for m in matches {
        println!("Trade: {} @ ${:.2}", m.quantity, m.price as f64 / 100.0);
    }
    
    // 5. Query order book
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    println!("Best bid: {:?}, Best ask: {:?}", best_bid, best_ask);
    
    Ok(())
}
```
