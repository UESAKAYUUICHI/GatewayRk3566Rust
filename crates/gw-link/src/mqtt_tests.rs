use super::*;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}};

async fn packet(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let header = stream.read_u8().await.unwrap();
    let (mut length, mut multiplier) = (0, 1);
    loop {
        let byte = stream.read_u8().await.unwrap();
        length += usize::from(byte & 127) * multiplier;
        if byte & 128 == 0 { break; }
        multiplier *= 128;
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();
    (header, body)
}

async fn receive_publish(stream: &mut TcpStream) -> u16 {
    loop {
        let (header, body) = packet(stream).await;
        match header >> 4 {
            8 => {
                // Both subscriptions are sent as individual MQTT SUBSCRIBE packets.
                stream.write_all(&[0x90, 3, body[0], body[1], 1]).await.unwrap();
            }
            3 => {
                let topic_length = usize::from(u16::from_be_bytes([body[0], body[1]]));
                return u16::from_be_bytes([body[topic_length + 2], body[topic_length + 3]]);
            }
            _ => {}
        }
    }
}

async fn link(port: u16) -> (Arc<MqttLink>, tokio::task::JoinHandle<()>) {
    let (commands, _receiver) = mpsc::channel(8);
    let (link, task) = MqttLink::spawn(LinkOptions {
        host: "127.0.0.1".into(), port, username: "".into(), password: "".into(),
        client_id: format!("ack-test-{port}"), gateway_id: "test".into(),
    }, "gateway/test/command/down".into(), commands);
    let link = Arc::new(link);
    tokio::time::timeout(Duration::from_secs(5), async {
        while link.health() != LinkHealth::Connected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    (link, task)
}

#[tokio::test]
async fn waits_for_the_wire_puback() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = oneshot::channel();
    let (ack_tx, ack_rx) = oneshot::channel();
    let broker = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(packet(&mut stream).await.0 >> 4, 1);
        stream.write_all(&[0x20, 2, 0, 0]).await.unwrap();
        let id = receive_publish(&mut stream).await;
        seen_tx.send(()).unwrap();
        ack_rx.await.unwrap();
        let [high, low] = id.to_be_bytes();
        stream.write_all(&[0x40, 2, high, low]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    let (link, task) = link(port).await;
    let published = link.clone();
    let mut result = tokio::spawn(async move { published.publish("gateway/test/data/up", b"sample").await });
    tokio::time::timeout(Duration::from_secs(5), seen_rx).await.unwrap().unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(100), &mut result).await.is_err());
    ack_tx.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5), result).await.unwrap().unwrap().is_ok());
    task.abort();
    broker.await.unwrap();
}

#[tokio::test]
async fn socket_close_cannot_confirm_delivery() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = oneshot::channel();
    let broker = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        packet(&mut stream).await;
        stream.write_all(&[0x20, 2, 0, 0]).await.unwrap();
        receive_publish(&mut stream).await;
        drop(stream);
        seen_tx.send(()).unwrap();
    });
    let (link, task) = link(port).await;
    let published = link.clone();
    let mut result = tokio::spawn(async move { published.publish("gateway/test/data/up", b"sample").await });
    tokio::time::timeout(Duration::from_secs(5), seen_rx).await.unwrap().unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(150), &mut result).await.is_err());
    result.abort();
    task.abort();
    broker.await.unwrap();
}
