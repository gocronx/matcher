use crate::types::{Order, OrderId, Price, Quantity, Side, MatchResult, ProductId, Timestamp, OrderType};
use crate::utils::HighResTimer;
use parking_lot::RwLock;
use ahash::AHashMap;
use smallvec::SmallVec;
use std::collections::BTreeMap;
use uuid::Uuid;

/// 价格层级 - 使用 SmallVec 优化小订单场景
#[derive(Debug, Clone)]
pub struct PriceLevel {
    pub price: Price,
    pub total_quantity: Quantity,
    pub visible_quantity: Quantity,  // 用于 Iceberg 订单
    pub order_count: u32,
    // 大多数价格层级订单数 < 8，使用 SmallVec 避免堆分配
    pub orders: SmallVec<[OrderId; 8]>,
}

impl PriceLevel {
    fn new(price: Price) -> Self {
        Self {
            price,
            total_quantity: 0,
            visible_quantity: 0,
            order_count: 0,
            orders: SmallVec::new(),
        }
    }
    
    fn add_order(&mut self, order_id: OrderId, quantity: Quantity, visible: Quantity) {
        self.orders.push(order_id);
        self.total_quantity += quantity;
        self.visible_quantity += visible;
        self.order_count += 1;
    }
    
    fn remove_order(&mut self, order_id: OrderId, quantity: Quantity, visible: Quantity) -> bool {
        if let Some(pos) = self.orders.iter().position(|&id| id == order_id) {
            self.orders.remove(pos);
            self.total_quantity = self.total_quantity.saturating_sub(quantity);
            self.visible_quantity = self.visible_quantity.saturating_sub(visible);
            self.order_count = self.order_count.saturating_sub(1);
            true
        } else {
            false
        }
    }
    
    fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

/// 高性能订单簿 - 集成 matching-core 的优化
/// 
/// 特性:
/// - ahash 快速哈希
/// - SmallVec 减少堆分配
/// - 支持所有高级订单类型
/// - Post-Only, Iceberg, Stop 订单
pub struct OrderBook {
    product_id: ProductId,
    
    // 使用 ahash 的 HashMap - 性能提升 20-30%
    orders: RwLock<AHashMap<OrderId, Order>>,
    
    // 价格层级 - BTreeMap 保持有序
    bids: RwLock<BTreeMap<Price, PriceLevel>>,
    asks: RwLock<BTreeMap<Price, PriceLevel>>,
    
    // 止损单池（未触发）
    stop_orders: RwLock<Vec<Order>>,
    
    // 最优价格缓存
    best_bid: RwLock<Option<Price>>,
    best_ask: RwLock<Option<Price>>,
    
    // 最新成交价（用于触发止损单）
    last_trade_price: RwLock<Option<Price>>,
    
    // 统计信息
    total_orders: RwLock<u64>,
    total_volume: RwLock<u64>,
}

impl OrderBook {
    /// 创建新的订单簿
    pub fn new(product_id: ProductId) -> Self {
        Self {
            product_id,
            orders: RwLock::new(AHashMap::with_capacity(1024)),
            bids: RwLock::new(BTreeMap::new()),
            asks: RwLock::new(BTreeMap::new()),
            stop_orders: RwLock::new(Vec::new()),
            best_bid: RwLock::new(None),
            best_ask: RwLock::new(None),
            last_trade_price: RwLock::new(None),
            total_orders: RwLock::new(0),
            total_volume: RwLock::new(0),
        }
    }
    
    /// 添加订单
    pub fn add_order(&self, order: Order) -> Result<(), String> {
        if order.product_id != self.product_id {
            return Err(format!("Product ID mismatch"));
        }
        
        let order_id = order.id;
        let price = order.price;
        let quantity = order.remaining_quantity();
        let side = order.side;
        
        // 计算可见数量（Iceberg 订单特殊处理）
        let visible_quantity = match order.order_type {
            OrderType::Iceberg { visible_size } => visible_size.min(quantity),
            _ => quantity,
        };
        
        // 添加到订单映射
        {
            let mut orders = self.orders.write();
            orders.insert(order_id, order);
        }
        
        // 添加到相应的价格层级
        match side {
            Side::Buy => {
                let mut bids = self.bids.write();
                let level = bids.entry(price).or_insert_with(|| PriceLevel::new(price));
                level.add_order(order_id, quantity, visible_quantity);
                
                // 更新最优买价
                let mut best_bid = self.best_bid.write();
                if best_bid.is_none() || price > best_bid.unwrap() {
                    *best_bid = Some(price);
                }
            }
            Side::Sell => {
                let mut asks = self.asks.write();
                let level = asks.entry(price).or_insert_with(|| PriceLevel::new(price));
                level.add_order(order_id, quantity, visible_quantity);
                
                // 更新最优卖价
                let mut best_ask = self.best_ask.write();
                if best_ask.is_none() || price < best_ask.unwrap() {
                    *best_ask = Some(price);
                }
            }
        }
        
        // 更新统计
        {
            let mut total_orders = self.total_orders.write();
            *total_orders += 1;
        }
        {
            let mut total_volume = self.total_volume.write();
            *total_volume += quantity;
        }
        
        Ok(())
    }
    
    /// 检查订单是否会立即成交（用于 Post-Only）
    fn would_match_immediately(&self, order: &Order) -> bool {
        match order.side {
            Side::Buy => {
                if let Some(best_ask) = *self.best_ask.read() {
                    order.price >= best_ask
                } else {
                    false
                }
            }
            Side::Sell => {
                if let Some(best_bid) = *self.best_bid.read() {
                    order.price <= best_bid
                } else {
                    false
                }
            }
        }
    }
    
    /// 处理止损单触发
    fn process_stop_orders(&self, current_price: Price) -> Vec<Order> {
        let mut triggered = Vec::new();
        let mut stop_orders = self.stop_orders.write();
        
        stop_orders.retain(|order| {
            let should_trigger = match (&order.order_type, order.side) {
                (OrderType::Stop { stop_price }, Side::Buy) => current_price >= *stop_price,
                (OrderType::Stop { stop_price }, Side::Sell) => current_price <= *stop_price,
                (OrderType::StopLimit { stop_price }, Side::Buy) => current_price >= *stop_price,
                (OrderType::StopLimit { stop_price }, Side::Sell) => current_price <= *stop_price,
                _ => false,
            };
            
            if should_trigger {
                triggered.push(order.clone());
                false  // 从止损池中移除
            } else {
                true  // 保留
            }
        });
        
        triggered
    }
    
    /// 撮合订单 - 支持所有高级订单类型
    pub fn match_order(&self, mut new_order: Order, current_time: Timestamp) -> Vec<MatchResult> {
        let mut matches = Vec::new();
        let timer = HighResTimer::start();
        
        // Post-Only 检查：如果会立即成交则拒绝
        if matches!(new_order.order_type, OrderType::PostOnly) {
            if self.would_match_immediately(&new_order) {
                return matches;  // 拒绝订单
            }
        }
        
        // 止损单：加入止损池，不立即撮合
        if matches!(new_order.order_type, OrderType::Stop { .. } | OrderType::StopLimit { .. }) {
            let mut stop_orders = self.stop_orders.write();
            stop_orders.push(new_order);
            return matches;
        }
        
        // FOK 检查：必须能完全成交
        if matches!(new_order.order_type, OrderType::FOK) {
            if !self.can_fill_completely(&new_order) {
                return matches;  // 拒绝订单
            }
        }
        
        // 正常撮合
        match new_order.side {
            Side::Sell => {
                matches.extend(self.match_against_bids(&mut new_order, current_time, &timer));
            }
            Side::Buy => {
                matches.extend(self.match_against_asks(&mut new_order, current_time, &timer));
            }
        }
        
        // 更新最新成交价并触发止损单
        if !matches.is_empty() {
            *self.last_trade_price.write() = Some(matches.last().unwrap().price);
            
            // 触发止损单
            let triggered = self.process_stop_orders(matches.last().unwrap().price);
            for stop_order in triggered {
                // 递归处理触发的止损单
                let stop_matches = self.match_order(stop_order, current_time);
                matches.extend(stop_matches);
            }
        }
        
        // 处理剩余订单
        if new_order.remaining_quantity() > 0 {
            match new_order.order_type {
                OrderType::Limit | OrderType::PostOnly => {
                    // 限价单和 Post-Only 加入订单簿
                    let _ = self.add_order(new_order);
                }
                OrderType::Iceberg { visible_size } => {
                    // Iceberg 订单：补充可见数量
                    if new_order.hidden_quantity > 0 {
                        let refill = new_order.hidden_quantity.min(visible_size);
                        new_order.quantity += refill;
                        new_order.hidden_quantity -= refill;
                    }
                    let _ = self.add_order(new_order);
                }
                OrderType::IOC | OrderType::FOK | OrderType::Market => {
                    // IOC, FOK, Market 不加入订单簿
                }
                _ => {}
            }
        }
        
        matches
    }
    
    /// 检查是否能完全成交（FOK）
    fn can_fill_completely(&self, order: &Order) -> bool {
        let (levels, is_buy) = match order.side {
            Side::Buy => (self.asks.read(), true),
            Side::Sell => (self.bids.read(), false),
        };
        
        let mut available = 0u64;
        
        for (price, level) in levels.iter() {
            // 价格检查
            if is_buy && *price > order.price {
                break;
            }
            if !is_buy && *price < order.price {
                break;
            }
            
            available += level.total_quantity;
            if available >= order.remaining_quantity() {
                return true;
            }
        }
        
        false
    }
    
    /// 对买单撮合
    fn match_against_bids(&self, new_order: &mut Order, current_time: Timestamp, timer: &HighResTimer) -> Vec<MatchResult> {
        let mut matches = Vec::new();
        
        // 收集需要撮合的价格
        let prices: Vec<Price> = {
            let bids = self.bids.read();
            if new_order.order_type == OrderType::Market {
                bids.keys().rev().cloned().collect()
            } else {
                bids.range(new_order.price..)
                    .rev()
                    .map(|(p, _)| *p)
                    .collect()
            }
        };
        
        for price in prices {
            if new_order.remaining_quantity() == 0 {
                break;
            }
            
            let matched = self.match_at_price_level(
                price,
                new_order,
                false,  // 对买单撮合
                current_time,
                timer,
            );
            matches.extend(matched);
        }
        
        matches
    }
    
    /// 对卖单撮合
    fn match_against_asks(&self, new_order: &mut Order, current_time: Timestamp, timer: &HighResTimer) -> Vec<MatchResult> {
        let mut matches = Vec::new();
        
        // 收集需要撮合的价格
        let prices: Vec<Price> = {
            let asks = self.asks.read();
            if new_order.order_type == OrderType::Market {
                asks.keys().cloned().collect()
            } else {
                asks.range(..=new_order.price)
                    .map(|(p, _)| *p)
                    .collect()
            }
        };
        
        for price in prices {
            if new_order.remaining_quantity() == 0 {
                break;
            }
            
            let matched = self.match_at_price_level(
                price,
                new_order,
                true,  // 对卖单撮合
                current_time,
                timer,
            );
            matches.extend(matched);
        }
        
        matches
    }
    
    /// 在特定价格层级撮合
    fn match_at_price_level(
        &self,
        price: Price,
        new_order: &mut Order,
        match_against_asks: bool,
        current_time: Timestamp,
        timer: &HighResTimer,
    ) -> Vec<MatchResult> {
        let mut matches = Vec::new();
        
        // 获取该价格层级的订单
        let order_ids: SmallVec<[OrderId; 8]> = {
            let levels = if match_against_asks {
                self.asks.read()
            } else {
                self.bids.read()
            };
            
            if let Some(level) = levels.get(&price) {
                level.orders.clone()
            } else {
                return matches;
            }
        };
        
        // 逐个撮合
        for order_id in order_ids {
            if new_order.remaining_quantity() == 0 {
                break;
            }
            
            // 获取resting订单
            let mut resting_order = {
                let orders = self.orders.read();
                match orders.get(&order_id) {
                    Some(order) => order.clone(),
                    None => continue,
                }
            };
            
            // 检查订单是否过期
            if !resting_order.is_active(current_time) {
                let _ = self.remove_order(order_id);
                continue;
            }
            
            // 计算成交数量
            let match_quantity = new_order.remaining_quantity()
                .min(resting_order.remaining_quantity());
            
            if match_quantity == 0 {
                continue;
            }
            
            // 更新订单
            new_order.filled_quantity += match_quantity;
            resting_order.filled_quantity += match_quantity;
            
            // 生成成交记录
            let (buy_order_id, sell_order_id) = match new_order.side {
                Side::Buy => (new_order.id, resting_order.id),
                Side::Sell => (resting_order.id, new_order.id),
            };
            
            matches.push(MatchResult {
                trade_id: Uuid::new_v4(),
                product_id: self.product_id.clone(),
                buy_order_id,
                sell_order_id,
                price,
                quantity: match_quantity,
                trade_time: current_time,
                match_latency_ns: timer.elapsed_ns(),
                aggressor_side: new_order.side,
            });
            
            // 更新 resting 订单
            {
                let mut orders = self.orders.write();
                if resting_order.is_filled() {
                    orders.remove(&order_id);
                } else {
                    orders.insert(order_id, resting_order.clone());
                }
            }
            
            // 如果完全成交，从价格层级移除
            if resting_order.is_filled() {
                let visible = match resting_order.order_type {
                    OrderType::Iceberg { visible_size } => visible_size,
                    _ => resting_order.quantity,
                };
                
                let mut levels = if match_against_asks {
                    self.asks.write()
                } else {
                    self.bids.write()
                };
                
                if let Some(level) = levels.get_mut(&price) {
                    level.remove_order(order_id, resting_order.quantity, visible);
                    if level.is_empty() {
                        levels.remove(&price);
                    }
                }
            }
        }
        
        // 更新最优价格
        self.update_best_prices();
        
        matches
    }
    
    /// 更新最优价格
    fn update_best_prices(&self) {
        {
            let asks = self.asks.read();
            let mut best_ask = self.best_ask.write();
            *best_ask = asks.keys().next().copied();
        }
        {
            let bids = self.bids.read();
            let mut best_bid = self.best_bid.write();
            *best_bid = bids.keys().next_back().copied();
        }
    }
    
    /// 移除订单
    pub fn remove_order(&self, order_id: OrderId) -> Result<Order, String> {
        let order = {
            let mut orders = self.orders.write();
            orders.remove(&order_id).ok_or_else(|| format!("Order not found"))?
        };
        
        let price = order.price;
        let quantity = order.remaining_quantity();
        let side = order.side;
        
        let visible = match order.order_type {
            OrderType::Iceberg { visible_size } => visible_size.min(quantity),
            _ => quantity,
        };
        
        match side {
            Side::Buy => {
                let mut bids = self.bids.write();
                if let Some(level) = bids.get_mut(&price) {
                    level.remove_order(order_id, quantity, visible);
                    if level.is_empty() {
                        bids.remove(&price);
                        let mut best_bid = self.best_bid.write();
                        if Some(price) == *best_bid {
                            *best_bid = bids.keys().next_back().copied();
                        }
                    }
                }
            }
            Side::Sell => {
                let mut asks = self.asks.write();
                if let Some(level) = asks.get_mut(&price) {
                    level.remove_order(order_id, quantity, visible);
                    if level.is_empty() {
                        asks.remove(&price);
                        let mut best_ask = self.best_ask.write();
                        if Some(price) == *best_ask {
                            *best_ask = asks.keys().next().copied();
                        }
                    }
                }
            }
        }
        
        Ok(order)
    }
    
    pub fn best_bid(&self) -> Option<Price> {
        *self.best_bid.read()
    }
    
    pub fn best_ask(&self) -> Option<Price> {
        *self.best_ask.read()
    }
    
    pub fn spread(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.saturating_sub(bid)),
            _ => None,
        }
    }
    
    pub fn depth(&self) -> (usize, usize) {
        let bid_count = self.bids.read().values().map(|l| l.order_count as usize).sum();
        let ask_count = self.asks.read().values().map(|l| l.order_count as usize).sum();
        (bid_count, ask_count)
    }
    
    pub fn total_orders(&self) -> u64 {
        *self.total_orders.read()
    }
    
    pub fn total_volume(&self) -> u64 {
        *self.total_volume.read()
    }
    
    /// 获取订单簿快照
    pub fn snapshot(&self, depth: usize) -> (Vec<crate::types::BookLevel>, Vec<crate::types::BookLevel>) {
        let bids = self.bids.read();
        let asks = self.asks.read();
        
        let bid_levels: Vec<crate::types::BookLevel> = bids.values()
            .rev()
            .take(depth)
            .map(|level| crate::types::BookLevel {
                price: level.price,
                quantity: level.total_quantity,
                order_count: level.order_count,
            })
            .collect();
            
        let ask_levels: Vec<crate::types::BookLevel> = asks.values()
            .take(depth)
            .map(|level| crate::types::BookLevel {
                price: level.price,
                quantity: level.total_quantity,
                order_count: level.order_count,
            })
            .collect();
        
        (bid_levels, ask_levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;
    use crate::utils::current_timestamp_ns;

    #[test]
    fn test_order_book_creation() {
        let book = OrderBook::new("BTC-USD".to_string());
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.total_orders(), 0);
    }

    #[tokio::test]
    async fn test_add_order() {
        let book = OrderBook::new("BTC-USD".to_string());
        let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
        
        let result = book.add_order(order);
        assert!(result.is_ok());
        assert_eq!(book.total_orders(), 1);
    }

    #[tokio::test]
    async fn test_post_only() {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // 先挂一个卖单
        let sell = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, current_timestamp_ns());
        book.add_order(sell).unwrap();
        
        // Post-Only 买单会被拒绝（因为会立即成交）
        let post_only = Order::post_only("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
        let matches = book.match_order(post_only, current_timestamp_ns());
        
        assert_eq!(matches.len(), 0);  // 应该被拒绝
    }

    #[tokio::test]
    async fn test_iceberg() {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Iceberg 订单
        let iceberg = Order::iceberg("BTC-USD".to_string(), Side::Sell, 50000, 1000, 100, current_timestamp_ns());
        book.add_order(iceberg).unwrap();
        
        // 检查可见数量
        let (_, asks) = book.snapshot(1);
        assert_eq!(asks[0].quantity, 100);  // 显示数量为 100
        assert_eq!(asks[0].order_count, 1);  // 1个订单
    }
}