//! `EventBus` 广播订阅与事件投递测试。

use agent::event_bus::*;

#[tokio::test]
async fn test_broadcast_to_multiple_subscribers() {
    let bus = EventBus::new(64);
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();

    bus.publish(AgentEvent::Token("Hello".to_string())).await;

    let e1 = rx1.recv().await.unwrap();
    let e2 = rx2.recv().await.unwrap();
    assert!(matches!(e1, AgentEvent::Token(_)));
    assert!(matches!(e2, AgentEvent::Token(_)));
}
