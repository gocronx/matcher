# Matcher - 高性能交易引擎

[English Documentation](README.md)

---

高性能交易撮合引擎，使用 Rust 构建，支持多种订单类型和异步处理。

## 特性

### 核心功能
- **高性能撮合引擎**：支持亚微秒级延迟的订单撮合
- **多种订单类型**：Market、Limit、IOC、FOK、Post-Only、Iceberg、GTD、Day
- **异步架构**：基于 Tokio 的异步运行时，支持高并发
- **内存优化**：对象池预分配、SmallVec 减少堆分配、AHash 快速哈希
- **完整监控**：Prometheus 指标集成、结构化日志

### 技术亮点
- **无锁算法**：减少竞争，提高吞吐量
- **SIMD 优化**：利用现代 CPU 特性
- **零拷贝网络**：最小化内存分配
- **配置驱动**：TOML 配置文件支持
- **API 优先设计**：REST 和 WebSocket 接口

## 快速开始

### 安装

```bash
git clone <repository-url>
cd matcher
cargo build --release
```

### 基础使用

```rust
use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建配置
    let config = Config::default();
    
    // 创建并启动引擎
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    
    let matching_engine = engine.matching_engine();
    
    // 提交限价单
    let order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50000,  // 价格: $500.00
        100,    // 数量: 100
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(order).await?;
    
    // 查看成交结果
    for m in matches {
        println!("成交: {} @ {}", m.quantity, m.price);
    }
    
    Ok(())
}
```

### 配置文件

创建 `config.toml` 文件：

```toml
[engine]
product_ids = ["BTC-USD", "ETH-USD"]
max_orders_per_product = 1000000

[network]
listen_port = 8080
multicast_addr = "239.0.0.1:5000"
broadcast_addr = "239.0.0.2:5001"

[performance]
batch_size = 100
worker_threads = 4
use_fast_hash = true
use_object_pool = true
use_smallvec = true
```

## 订单类型详解

### 1. 市价单 (Market Order)

立即以最优价格成交的订单。

```rust
let market_order = Order::market(
    "BTC-USD".to_string(),
    Side::Buy,
    100,  // 数量
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(market_order).await?;
```

### 2. 限价单 (Limit Order)

指定价格的订单，只在价格达到或优于指定价格时成交。

```rust
let limit_order = Order::limit(
    "BTC-USD".to_string(),
    Side::Sell,
    50100,  // 价格: $501.00
    200,    // 数量: 200
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(limit_order).await?;
```

### 3. Post-Only 订单（只做 Maker）

只添加流动性的订单，如果会立即成交则被拒绝。

```rust
let post_only = Order::post_only(
    "BTC-USD".to_string(),
    Side::Buy,
    49900,  // 价格
    100,    // 数量
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(post_only).await?;
// 如果订单会立即成交，matches 将为空，订单被拒绝
```

### 4. 冰山单 (Iceberg Order)

隐藏真实订单量的大额订单。

```rust
let iceberg = Order::iceberg(
    "BTC-USD".to_string(),
    Side::Sell,
    50100,  // 价格
    1000,   // 总数量
    100,    // 显示数量
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(iceberg).await?;
// 订单簿中只显示 100，但总量为 1000
```

## API 参考

### Engine API

```rust
// 创建引擎
let mut engine = Engine::new(config).await?;

// 启动引擎
engine.start().await?;

// 获取撮合引擎
let matching_engine = engine.matching_engine();
```

### MatchingEngine API

```rust
// 提交订单
let matches = matching_engine.submit_order(order).await?;

// 取消订单
matching_engine.cancel_order(&order_id).await?;

// 获取最优买卖价
let (best_bid, best_ask) = matching_engine.get_best_prices(&product_id)?;

// 获取价差
let spread = matching_engine.get_spread(&product_id)?;

// 获取订单簿快照
let (bids, asks) = matching_engine.get_order_book_snapshot(&product_id, 10)?;

// 获取市场深度
let (bid_depth, ask_depth) = matching_engine.get_depth(&product_id)?;

// 获取引擎统计
let stats = matching_engine.get_stats();
```

### Order 构造方法

```rust
// 市价单
Order::market(product_id, side, quantity, timestamp)

// 限价单
Order::limit(product_id, side, price, quantity, timestamp)

// Post-Only 订单
Order::post_only(product_id, side, price, quantity, timestamp)

// 冰山单
Order::iceberg(product_id, side, price, total_quantity, visible_quantity, timestamp)
```

## 运行示例

### 基础撮合演示

```bash
cargo run --example basic_usage --release
```

输出示例：
```
Starting Matcher Example
Engine started, submitting sample orders...
Submitted orders 1: buy_matches=0, sell_matches=0
...
Market order generated 2 matches:
  Match 1: 100 units at $501.00 (latency: 823ns)
  Match 2: 50 units at $502.00 (latency: 645ns)
```

### 高级特性演示

```bash
cargo run --example advanced_features --release
```

输出示例：
```
Matcher v2.0 - 高级特性演示
引擎启动完成，性能优化已启用
   - Fast Hash (ahash): ✓
   - Object Pool: ✓
   - SmallVec: ✓

演示 1: Post-Only 订单（只做 Maker）
   Post-Only 订单提交: 0 笔成交

演示 2: Iceberg 订单（冰山单）
   Iceberg 订单: 总量 1000, 显示 100
   成交: 0 笔
```

### 完整演示

```bash
cargo run --example complete_demo --release
```

## 性能基准测试

### 运行基准测试

```bash
# 运行所有基准测试
cargo bench

# 运行特定基准测试
cargo bench --bench matching_benchmark
```

### 性能指标

| 指标 | Matcher | 说明 |
|------|---------|------|
| 延迟 (p50) | ~800ns | 中位数延迟 |
| 延迟 (p99) | ~2μs | 99分位延迟 |
| 吞吐量 | 2M ops/sec | 每秒操作数 |
| 内存占用 | ~50MB | 100万订单 |

## 测试

```bash
# 运行所有测试
cargo test --release

# 运行单元测试
cargo test --lib

# 运行集成测试
cargo test --test integration

# 查看测试覆盖率
cargo tarpaulin --out Html
```

## 项目结构

```
matcher/
├── src/
│   ├── lib.rs           # 库入口
│   ├── main.rs          # 可执行文件入口
│   ├── config/          # 配置模块
│   │   └── mod.rs       # 配置定义
│   ├── core/            # 核心引擎
│   │   ├── engine.rs    # 主引擎
│   │   ├── matching.rs  # 撮合引擎
│   │   └── orderbook.rs # 订单簿
│   ├── network/         # 网络模块
│   │   └── mod.rs       # 网络处理
│   ├── types/           # 类型定义
│   │   └── mod.rs       # 订单、成交等类型
│   └── utils/           # 工具模块
│       ├── metrics.rs   # 指标收集
│       ├── timer.rs     # 高精度计时器
│       └── mod.rs       # 工具函数
├── examples/            # 示例代码
│   ├── basic_usage.rs       # 基础使用
│   ├── advanced_features.rs # 高级特性
│   └── complete_demo.rs     # 完整演示
├── benches/             # 基准测试
│   └── matching_benchmark.rs
├── tests/               # 集成测试
│   └── integration.rs
├── config.toml          # 配置文件
├── Cargo.toml           # 项目配置
└── README.md            # 本文件
```

## 监控与指标

Matcher 提供完整的监控支持：

### Prometheus 指标

- `matcher_orders_received_total` - 接收订单总数
- `matcher_orders_matched_total` - 撮合订单总数
- `matcher_trades_executed_total` - 成交笔数
- `matcher_match_latency_seconds` - 撮合延迟分布
- `matcher_order_book_depth` - 订单簿深度
- `matcher_spread` - 买卖价差

### 结构化日志

```rust
use tracing::{info, warn, error};

info!("Order submitted: {:?}", order);
warn!("Order rejected: {:?}", reason);
error!("Engine error: {:?}", error);
```

## 依赖项

主要依赖：
- `tokio`: 异步运行时
- `serde`: 序列化框架
- `ahash`: 快速哈希算法
- `smallvec`: 小向量优化
- `slab`: 对象池
- `prometheus`: 指标收集
- `tracing`: 结构化日志

## 开发指南

### 构建

```bash
# Debug 构建
cargo build

# Release 构建（优化）
cargo build --release

# 检查代码
cargo check

# 格式化代码
cargo fmt

# 代码检查
cargo clippy
```

### 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详情。

## 常见问题

### Q: 如何提高性能？

A: 
1. 使用 `--release` 模式编译
2. 启用性能优化选项（fast_hash, object_pool, smallvec）
3. 调整 `batch_size` 和 `worker_threads`
4. 使用 SSD 存储 WAL 日志

### Q: 支持哪些订单类型？

A: 目前支持：
- Market（市价单）
- Limit（限价单）
- IOC（立即成交或取消）
- FOK（全部成交或全部取消）
- Post-Only（只做 Maker）
- Iceberg（冰山单）
- GTD（指定日期过期）
- Day（当日有效）

### Q: 如何集成到现有系统？

A: Matcher 提供多种集成方式：
1. 作为 Rust 库使用
2. 通过 REST API 调用
3. 通过 WebSocket 实时通信
4. 通过消息队列集成