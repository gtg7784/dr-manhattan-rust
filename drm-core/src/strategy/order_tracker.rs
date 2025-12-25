use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::models::{Order, OrderStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderEvent {
    Created,
    PartialFill,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone)]
pub struct TrackedOrder {
    pub order: Order,
    pub total_filled: f64,
    pub created_time: DateTime<Utc>,
}

impl TrackedOrder {
    pub fn new(order: Order) -> Self {
        let filled = order.filled;
        Self {
            order,
            total_filled: filled,
            created_time: Utc::now(),
        }
    }
}

pub type OrderCallback = Arc<dyn Fn(OrderEvent, &Order, f64) + Send + Sync>;

pub struct OrderTracker {
    tracked_orders: RwLock<HashMap<String, TrackedOrder>>,
    callbacks: RwLock<Vec<OrderCallback>>,
    verbose: bool,
}

impl OrderTracker {
    pub fn new(verbose: bool) -> Self {
        Self {
            tracked_orders: RwLock::new(HashMap::new()),
            callbacks: RwLock::new(Vec::new()),
            verbose,
        }
    }

    pub fn on_fill<F>(&self, callback: F) -> &Self
    where
        F: Fn(OrderEvent, &Order, f64) + Send + Sync + 'static,
    {
        let mut callbacks = self.callbacks.write().unwrap();
        callbacks.push(Arc::new(callback));
        self
    }

    pub fn track_order(&self, order: Order) {
        let order_id = order.id.clone();
        let mut tracked = self.tracked_orders.write().unwrap();

        if tracked.contains_key(&order_id) {
            return;
        }

        if self.verbose {
            let id_preview = if order_id.len() > 16 {
                &order_id[..16]
            } else {
                &order_id
            };
            println!("Tracking order {}...", id_preview);
        }

        tracked.insert(order_id, TrackedOrder::new(order));
    }

    pub fn untrack_order(&self, order_id: &str) {
        let mut tracked = self.tracked_orders.write().unwrap();
        tracked.remove(order_id);
    }

    pub fn handle_trade(&self, order_id: &str, fill_size: f64, fill_price: f64, market_id: Option<&str>, outcome: Option<&str>) {
        let (event, updated_order) = {
            let mut tracked = self.tracked_orders.write().unwrap();

            let tracked_order = match tracked.get_mut(order_id) {
                Some(t) => t,
                None => return,
            };

            tracked_order.total_filled += fill_size;

            let is_complete = tracked_order.total_filled >= tracked_order.order.size;
            let new_status = if is_complete {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            };

            let updated_order = Order {
                id: tracked_order.order.id.clone(),
                market_id: market_id
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| tracked_order.order.market_id.clone()),
                outcome: outcome
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| tracked_order.order.outcome.clone()),
                side: tracked_order.order.side,
                price: fill_price,
                size: tracked_order.order.size,
                filled: tracked_order.total_filled,
                status: new_status,
                created_at: tracked_order.order.created_at,
                updated_at: Some(Utc::now()),
            };

            tracked_order.order = updated_order.clone();

            let event = if is_complete {
                OrderEvent::Filled
            } else {
                OrderEvent::PartialFill
            };

            (event, updated_order)
        };

        self.emit(event, &updated_order, fill_size);

        if event == OrderEvent::Filled {
            self.untrack_order(order_id);
        }
    }

    pub fn handle_cancel(&self, order_id: &str) {
        let order = {
            let tracked = self.tracked_orders.read().unwrap();
            tracked.get(order_id).map(|t| t.order.clone())
        };

        if let Some(order) = order {
            self.emit(OrderEvent::Cancelled, &order, 0.0);
            self.untrack_order(order_id);
        }
    }

    fn emit(&self, event: OrderEvent, order: &Order, fill_size: f64) {
        let callbacks = self.callbacks.read().unwrap();
        for callback in callbacks.iter() {
            callback(event, order, fill_size);
        }
    }

    pub fn tracked_count(&self) -> usize {
        self.tracked_orders.read().unwrap().len()
    }

    pub fn get_tracked_orders(&self) -> Vec<Order> {
        self.tracked_orders
            .read()
            .unwrap()
            .values()
            .map(|t| t.order.clone())
            .collect()
    }

    pub fn clear(&self) {
        self.tracked_orders.write().unwrap().clear();
    }
}

impl Default for OrderTracker {
    fn default() -> Self {
        Self::new(false)
    }
}

pub fn create_fill_logger() -> impl Fn(OrderEvent, &Order, f64) + Send + Sync {
    move |event: OrderEvent, order: &Order, fill_size: f64| {
        let side_str = format!("{:?}", order.side).to_uppercase();

        match event {
            OrderEvent::Filled => {
                println!(
                    "FILLED {} {} {:.2} @ {:.4}",
                    order.outcome, side_str, fill_size, order.price
                );
            }
            OrderEvent::PartialFill => {
                println!(
                    "PARTIAL {} {} +{:.2} ({:.2}/{:.2}) @ {:.4}",
                    order.outcome, side_str, fill_size, order.filled, order.size, order.price
                );
            }
            OrderEvent::Cancelled => {
                println!(
                    "CANCELLED {} {} {:.2} @ {:.4} (filled: {:.2})",
                    order.outcome, side_str, order.size, order.price, order.filled
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OrderSide;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_test_order(id: &str, size: f64) -> Order {
        Order {
            id: id.to_string(),
            market_id: "test-market".to_string(),
            outcome: "Yes".to_string(),
            side: OrderSide::Buy,
            price: 0.50,
            size,
            filled: 0.0,
            status: OrderStatus::Open,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn test_track_order() {
        // given
        let tracker = OrderTracker::new(false);
        let order = make_test_order("order-1", 10.0);

        // when
        tracker.track_order(order);

        // then
        assert_eq!(tracker.tracked_count(), 1);
    }

    #[test]
    fn test_partial_fill() {
        // given
        let tracker = OrderTracker::new(false);
        let order = make_test_order("order-1", 10.0);
        tracker.track_order(order);

        let fill_count = Arc::new(AtomicUsize::new(0));
        let fill_count_clone = fill_count.clone();

        tracker.on_fill(move |event, _, _| {
            if event == OrderEvent::PartialFill {
                fill_count_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        // when
        tracker.handle_trade("order-1", 3.0, 0.50, None, None);

        // then
        assert_eq!(fill_count.load(Ordering::SeqCst), 1);
        assert_eq!(tracker.tracked_count(), 1);
    }

    #[test]
    fn test_complete_fill() {
        // given
        let tracker = OrderTracker::new(false);
        let order = make_test_order("order-1", 10.0);
        tracker.track_order(order);

        let filled = Arc::new(AtomicUsize::new(0));
        let filled_clone = filled.clone();

        tracker.on_fill(move |event, _, _| {
            if event == OrderEvent::Filled {
                filled_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        // when
        tracker.handle_trade("order-1", 10.0, 0.50, None, None);

        // then
        assert_eq!(filled.load(Ordering::SeqCst), 1);
        assert_eq!(tracker.tracked_count(), 0);
    }

    #[test]
    fn test_cancel() {
        // given
        let tracker = OrderTracker::new(false);
        let order = make_test_order("order-1", 10.0);
        tracker.track_order(order);

        let cancelled = Arc::new(AtomicUsize::new(0));
        let cancelled_clone = cancelled.clone();

        tracker.on_fill(move |event, _, _| {
            if event == OrderEvent::Cancelled {
                cancelled_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        // when
        tracker.handle_cancel("order-1");

        // then
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(tracker.tracked_count(), 0);
    }
}
