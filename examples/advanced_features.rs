use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let _ = tracing_subscriber::fmt::try_init();
    
    info!("Matcher v2.0 - 高级特性演示");
    
    // 创建配置 - 启用所有优化
    let mut config = Config::default();
    config.performance.use_fast_hash = true;
    config.performance.use_object_pool = true;
    config.performance.use_smallvec = true;
    
    // 创建引擎
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    
    let matching_engine = engine.matching_engine();
    
    info!("引擎启动完成，性能优化已启用");
    info!("- Fast Hash (ahash): ✓");
    info!("- Object Pool: ✓");
    info!("- SmallVec: ✓");
    
    // 演示 1: Post-Only 订单
    info!("\n演示 1: Post-Only 订单（只做 Maker）");
    
    let post_only = Order::post_only(
        "BTC-USD".to_string(),
        Side::Buy,
        49900,
        100,
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(post_only).await?;
    info!("Post-Only 订单提交: {} 笔成交", matches.len());
    
    // 演示 2: Iceberg 订单
    info!("\n演示 2: Iceberg 订单（冰山单）");
    
    let iceberg = Order::iceberg(
        "BTC-USD".to_string(),
        Side::Sell,
        50100,
        1000,  // 总量
        100,   // 显示量
        current_timestamp_ns(),
    );
    
    info!("Iceberg 订单: 总量 1000, 显示 100");
    let matches = matching_engine.submit_order(iceberg).await?;
    info!("成交: {} 笔", matches.len());
    
    // 演示 3: 普通限价单
    info!("\n演示 3: 普通限价单");
    
    for i in 0..5 {
        let buy = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            49800 + (i * 100),
            50,
            current_timestamp_ns(),
        );
        
        let sell = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50200 + (i * 100),
            50,
            current_timestamp_ns(),
        );
        
        matching_engine.submit_order(buy).await?;
        matching_engine.submit_order(sell).await?;
    }
    
    info!("已提交 10 笔限价单");
    
    // 显示订单簿状态
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    let spread = matching_engine.get_spread(&"BTC-USD".to_string())?;
    let (bid_depth, ask_depth) = matching_engine.get_depth(&"BTC-USD".to_string())?;
    
    info!("\n订单簿状态:");
    info!("最优买价: {:?}", best_bid);
    info!("最优卖价: {:?}", best_ask);
    info!("价差: {:?}", spread);
    info!("深度: {} 买单, {} 卖单", bid_depth, ask_depth);
    
    // 演示 4: 市价单撮合
    info!("\n演示 4: 市价单撮合");
    
    let market = Order::market(
        "BTC-USD".to_string(),
        Side::Buy,
        150,
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(market).await?;
    info!("市价单成交: {} 笔", matches.len());
    
    for (i, m) in matches.iter().enumerate() {
        info!("成交 {}: {} @ {} (延迟: {}ns)", 
              i + 1, m.quantity, m.price, m.match_latency_ns);
    }
    
    // 显示引擎统计
    let stats = matching_engine.get_stats();
    info!("\n引擎统计:");
    info!("订单总数: {}", stats.orders_received);
    info!("撮合订单: {}", stats.orders_matched);
    info!("成交笔数: {}", stats.trades_executed);
    info!("平均延迟: {}ns", stats.avg_match_latency_ns);
    info!("运行时间: {}s", stats.uptime_seconds);
    
    info!("\n演示完成！");
    info!("提示: 使用 --release 模式可获得最佳性能");
    
    Ok(())
}