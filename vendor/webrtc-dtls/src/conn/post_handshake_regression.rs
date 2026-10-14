//! Reader regression using the actual DTLS record parser and read_and_buffer.
//! No live capture, peer identity, private keys or cryptographic fixtures.
use super::*;
use crate::handshake::handshake_message_server_hello_done::HandshakeMessageServerHelloDone;
use crate::handshake::{Handshake, HandshakeMessage};
use util::conn::conn_pipe::pipe;

#[tokio::test]
async fn post_handshake_completion_unblocks_inactive_fsm_wait() {
    let (reader_pipe, writer_pipe) = pipe();
    let next_conn: Arc<dyn util::Conn + Send + Sync> = Arc::new(reader_pipe);
    let (decrypted_tx, _decrypted_rx) = mpsc::channel(1);
    // Keep this receiver alive but deliberately do not consume it: the FSM
    // returned Finished, while DTLSConn still owns its handshake_rx receiver.
    let (handshake_tx, handshake_rx) = mpsc::channel(1);
    let (completion_tx, completion_rx) = mpsc::channel(1);
    let (packet_tx, _packet_rx) = mpsc::channel(1);
    let (_handle_queue_tx, mut handle_queue_rx) = mpsc::channel(1);
    let mut ctx = ConnReaderContext {
        is_client: false,
        replay_protection_window: DEFAULT_REPLAY_PROTECTION_WINDOW,
        replay_detector: vec![],
        decrypted_tx,
        encrypted_packets: vec![],
        fragment_buffer: FragmentBuffer::new(),
        cache: HandshakeCache::new(),
        cipher_suite: Arc::new(Mutex::new(None)),
        remote_epoch: Arc::new(AtomicU16::new(0)),
        handshake_tx,
        handshake_done_rx: completion_rx,
        packet_tx: Arc::new(packet_tx),
    };
    let local_epoch = Arc::new(AtomicU16::new(0));
    let handshake_done = Arc::new(AtomicBool::new(true));
    let mut worker = tokio::spawn(async move {
        let mut buf = vec![0; INBOUND_BUFFER_SIZE];
        DTLSConn::read_and_buffer(
            &mut ctx, &next_conn, &mut handle_queue_rx, &mut buf,
            &local_epoch, &handshake_done,
        ).await
    });

    // A valid plaintext epoch0 handshake record exercises the real fragment
    // buffer/parser and has_handshake path. TLS authentication is outside this
    // reader-wait regression; the inactive FSM never inspects this fixture.
    let record = RecordLayer::new(
        PROTOCOL_VERSION1_2, 0,
        Content::Handshake(Handshake::new(HandshakeMessage::ServerHelloDone(
            HandshakeMessageServerHelloDone {},
        ))),
    );
    let mut packet = Vec::new();
    record.marshal(&mut packet).unwrap();
    writer_pipe.send(&packet).await.unwrap();

    // Holding completion_tx open makes outer select choose the ready send
    // branch deterministically, rather than relying on select's random order.
    let notification_ready = tokio::time::timeout(Duration::from_secs(1), async {
        while handshake_rx.len() != 1 {
            tokio::task::yield_now().await;
        }
    }).await;
    if notification_ready.is_err() {
        worker.abort();
        panic!("actual reader did not enqueue inactive-FSM notification");
    }
    assert!(!worker.is_finished());
    drop(completion_tx);

    // Original inner select has no completion arm, so this times out.
    // Fixed reader observes completion and releases its packet-read loop.
    let completed = tokio::time::timeout(Duration::from_millis(500), &mut worker).await;
    if completed.is_err() {
        worker.abort();
        panic!("post-handshake reader remained blocked on inactive FSM");
    }
    completed.unwrap().unwrap().unwrap();
}
