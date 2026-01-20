use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;
use tracing::info;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let _ = tracing_subscriber::fmt::try_init();
    
    info!("🚀 Matcher v2.0 - 完整功能演示");
    info!("参考 matching-core 的高级特性实现\n");
    
    // 创建配置 - 启用所有优化
    let mut config = Config::default();
    config.performance.use_fast_hash = true;
    config.performance.use_object_pool = true;
    config.performance.use_smallvec = true;
    
    // 创建引擎
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    
    let matching_engine = engine.matching_engine();
    
    info!("✅ 引擎启动完成");
    info!("   性能优化: ahash ✓ | SmallVec ✓ | ObjectPool ✓\n");
    
    // ==================== 演示 1: 基础限价单 ====================
    info!("📋 演示 1: 基础限价单撮合");
    info!("   场景: 构建订单簿深度");
    
    // 挂买单
    for i in 0..5 {
        let buy = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            49900 - (i * 100),
            100,
            current_timestamp_ns(),
        );
        matching_engine.submit_order(buy).await?;
    }
    
    // 挂卖单
    for i in 0..5 {
        let sell = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50100 + (i * 100),
            100,
            current_timestamp_ns(),
        );
        matching_engine.submit_order(sell).await?;
    }
    
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    info!("   ✓ 最优买价: {:?}, 最优卖价: {:?}", best_bid, best_ask);
    info!("   ✓ 价差: {:?}\n", matching_engine.get_spread(&"BTC-USD".to_string())?);
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 2: Post-Only 订单 ====================
    info!("🎯 演示 2: Post-Only 订单（只做 Maker）");
    info!("   场景: 避免吃单，只提供流动性");
    
    // 这个 Post-Only 会成功（不会立即成交）
    let post_only_ok = Order::post_only(
        "BTC-USD".to_string(),
        Side::Buy,
        49800,  // 低于最优卖价
        50,
        current_timestamp_ns(),
    );
    let matches = matching_engine.submit_order(post_only_ok).await?;
    info!("   ✓ Post-Only 买单 @49800: {} 笔成交（应为0）", matches.len());
    
    // 这个 Post-Only 会被拒绝（会立即成交）
    let post_only_reject = Order::post_only(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,  // 等于最优卖价
        50,
        current_timestamp_ns(),
    );
    let matches = matching_engine.submit_order(post_only_reject).await?;
    info!("   ✓ Post-Only 买单 @50100: {} 笔成交（应被拒绝）\n", matches.len());
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 3: Iceberg 订单 ====================
    info!("🧊 演示 3: Iceberg 订单（冰山单）");
    info!("   场景: 隐藏真实挂单量，避免市场冲击");
    
    let iceberg = Order::iceberg(
        "BTC-USD".to_string(),
        Side::Sell,
        50500,
        1000,  // 总量 1000
        100,   // 只显示 100
        current_timestamp_ns(),
    );
    
    info!("   ✓ Iceberg 卖单: 总量 1000, 显示 100");
    matching_engine.submit_order(iceberg).await?;
    
    let (_, asks) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 10)?;
    info!("   ✓ 订单簿显示: {} 个卖单档位", asks.len());
    info!("   ✓ 市场只能看到显示的 100，隐藏了 900\n");
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 4: IOC 订单 ====================
    info!("⚡ 演示 4: IOC 订单（立即成交或取消）");
    info!("   场景: 快速成交，不留挂单");
    
    // 先挂一些卖单
    for i in 0..3 {
        let sell = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50000 + (i * 10),
            50,
            current_timestamp_ns(),
        );
        matching_engine.submit_order(sell).await?;
    }
    
    // IOC 买单
    let mut ioc = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50020,
        200,  // 想买 200，但只能成交部分
        current_timestamp_ns(),
    );
    ioc.order_type = matcher::types::OrderType::IOC;
    
    let matches = matching_engine.submit_order(ioc).await?;
    info!("   ✓ IOC 订单成交: {} 笔", matches.len());
    for (i, m) in matches.iter().enumerate() {
        info!("     - 成交 {}: {} @ {}", i + 1, m.quantity, m.price);
    }
    info!("   ✓ 未成交部分自动取消\n");
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 5: FOK 订单 ====================
    info!("💥 演示 5: FOK 订单（全部成交或全部取消）");
    info!("   场景: 必须完全成交，否则取消");
    
    // FOK 失败案例
    let mut fok_fail = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,
        1000,  // 数量太大，无法完全成交
        current_timestamp_ns(),
    );
    fok_fail.order_type = matcher::types::OrderType::FOK;
    
    let matches = matching_engine.submit_order(fok_fail).await?;
    info!("   ✗ FOK 订单（1000）: {} 笔成交（流动性不足，被拒绝）", matches.len());
    
    // FOK 成功案例
    let mut fok_ok = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,
        50,  // 数量合适
        current_timestamp_ns(),
    );
    fok_ok.order_type = matcher::types::OrderType::FOK;
    
    let matches = matching_engine.submit_order(fok_ok).await?;
    info!("   ✓ FOK 订单（50）: {} 笔成交（完全成交）\n", matches.len());
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 6: 市价单 ====================
    info!("💰 演示 6: 市价单撮合");
    info!("   场景: 以最优价格立即成交");
    
    let market = Order::market(
        "BTC-USD".to_string(),
        Side::Buy,
        150,
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(market).await?;
    info!("   ✓ 市价单成交: {} 笔", matches.len());
    
    let mut total_qty = 0;
    let mut total_value = 0u64;
    for m in &matches {
        total_qty += m.quantity;
        total_value += m.price * m.quantity;
    }
    let avg_price = if total_qty > 0 { total_value / total_qty } else { 0 };
    info!("   ✓ 总成交量: {}, 平均价格: {}\n", total_qty, avg_price);
    
    sleep(Duration::from_millis(100)).await;
    
    // ==================== 演示 7: 订单取消 ====================
    info!("❌ 演示 7: 订单取消");
    info!("   场景: 取消未成交的订单");
    
    let order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        49000,
        100,
        current_timestamp_ns(),
    );
    let order_id = order.id;
    matching_engine.submit_order(order).await?;
    
    info!("   ✓ 提交订单: {}", order_id);
    
    let cancelled = matching_engine.cancel_order(&"BTC-USD".to_string(), order_id).await?;
    info!("   ✓ 取消订单: {}, 数量: {}\n", cancelled.id, cancelled.quantity);
    
    // ==================== 最终统计 ====================
    info!("📊 最终统计");
    
    let stats = matching_engine.get_stats();
    info!("   订单总数: {}", stats.orders_received);
    info!("   撮合订单: {}", stats.orders_matched);
    info!("   成交笔数: {}", stats.trades_executed);
    info!("   平均延迟: {}ns", stats.avg_match_latency_ns);
    info!("   运行时间: {}s", stats.uptime_seconds);
    
    let (bid_depth, ask_depth) = matching_engine.get_depth(&"BTC-USD".to_string())?;
    info!("   订单簿深度: {} 买单, {} 卖单", bid_depth, ask_depth);
    
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    info!("   最优价格: Bid {:?}, Ask {:?}", best_bid, best_ask);
    
    info!("\n✅ 演示完成！");
    info!("💡 提示:");
    info!("   - 使用 --release 模式获得最佳性能");
    info!("   - 参考 matching-core 的设计理念");
    info!("   - 支持 8 种订单类型: Market, Limit, IOC, FOK, Post-Only, Iceberg, Stop, StopLimit");
    
    Ok(())
}