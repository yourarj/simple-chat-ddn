use chat_core::{
  error::ApplicationError,
  protocol::{ClientMessage, LENGTH_PREFIX, ServerMessage, decode_message, encode_message},
  transport_layer::{read_message_from_stream, write_message_to_stream},
};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn test_protocol_message_round_trip() {
  let client_test_messages = [
    ClientMessage::Join {
      username: "alice".to_string(),
    },
    ClientMessage::Leave {
      username: "bob".to_string(),
    },
    ClientMessage::Message {
      username: "charlie".to_string(),
      content: "Hello, world!".to_string(),
    },
  ];

  for (i, original_message) in client_test_messages.iter().enumerate() {
    let encoded = encode_message(original_message)
      .unwrap_or_else(|e| panic!("Failed to encode client message {}: {}", i, e));

    assert!(
      encoded.len() > LENGTH_PREFIX,
      "Encoded client message {} should be longer than length prefix",
      i
    );

    let payload = &encoded[LENGTH_PREFIX..];
    let decoded: ClientMessage = decode_message(payload)
      .unwrap_or_else(|e| panic!("Failed to decode client message {}: {}", i, e));

    match (original_message, decoded) {
      (
        ClientMessage::Join {
          username: orig_user,
        },
        ClientMessage::Join { username: dec_user },
      ) => {
        assert_eq!(orig_user, &dec_user, "Join message {} username mismatch", i);
      }
      (
        ClientMessage::Leave {
          username: orig_user,
        },
        ClientMessage::Leave { username: dec_user },
      ) => {
        assert_eq!(
          orig_user, &dec_user,
          "Leave message {} username mismatch",
          i
        );
      }
      (
        ClientMessage::Message {
          username: orig_user,
          content: orig_content,
        },
        ClientMessage::Message {
          username: dec_user,
          content: dec_content,
        },
      ) => {
        assert_eq!(orig_user, &dec_user, "Message {} username mismatch", i);
        assert_eq!(orig_content, &dec_content, "Message {} content mismatch", i);
      }
      _ => panic!("Client message {} type mismatch after encoding/decoding", i),
    }
  }

  let server_test_messages = [
    ServerMessage::Success {
      message: "Welcome to the chat!".to_string(),
    },
    ServerMessage::Error {
      reason: "Username already taken".to_string(),
    },
    ServerMessage::Message {
      username: "dave".to_string(),
      content: "Test message".to_string(),
    },
    ServerMessage::UserJoined {
      username: "newuser".to_string(),
    },
    ServerMessage::UserLeft {
      username: "leavinguser".to_string(),
    },
  ];

  for (i, original_message) in server_test_messages.iter().enumerate() {
    let encoded = encode_message(original_message)
      .unwrap_or_else(|e| panic!("Failed to encode server message {}: {}", i, e));

    assert!(
      encoded.len() > LENGTH_PREFIX,
      "Encoded server message {} should be longer than length prefix",
      i
    );

    let payload = &encoded[LENGTH_PREFIX..];
    let decoded: ServerMessage = decode_message(payload)
      .unwrap_or_else(|e| panic!("Failed to decode server message {}: {}", i, e));

    match (original_message, decoded) {
      (
        ServerMessage::Success { message: orig_msg },
        ServerMessage::Success { message: dec_msg },
      ) => {
        assert_eq!(orig_msg, &dec_msg, "Success message {} mismatch", i);
      }
      (
        ServerMessage::Error {
          reason: orig_reason,
        },
        ServerMessage::Error { reason: dec_reason },
      ) => {
        assert_eq!(orig_reason, &dec_reason, "Error message {} mismatch", i);
      }
      (
        ServerMessage::Message {
          username: orig_user,
          content: orig_content,
        },
        ServerMessage::Message {
          username: dec_user,
          content: dec_content,
        },
      ) => {
        assert_eq!(
          orig_user, &dec_user,
          "Server message {} username mismatch",
          i
        );
        assert_eq!(
          orig_content, &dec_content,
          "Server message {} content mismatch",
          i
        );
      }
      (
        ServerMessage::UserJoined {
          username: orig_user,
        },
        ServerMessage::UserJoined { username: dec_user },
      ) => {
        assert_eq!(
          orig_user, &dec_user,
          "UserJoined message {} username mismatch",
          i
        );
      }
      (
        ServerMessage::UserLeft {
          username: orig_user,
        },
        ServerMessage::UserLeft { username: dec_user },
      ) => {
        assert_eq!(
          orig_user, &dec_user,
          "UserLeft message {} username mismatch",
          i
        );
      }
      _ => panic!("Server message {} type mismatch after encoding/decoding", i),
    }
  }
}

#[tokio::test]
async fn test_transport_layer_tcp_communication() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();

  let test_message = ClientMessage::Message {
    username: "testuser".to_string(),
    content: "Hello via TCP!".to_string(),
  };

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 4096];
    let received_message: ClientMessage = read_message_from_stream(&mut reader, &mut buffer)
      .await
      .unwrap();

    let response = ServerMessage::Success {
      message: format!(
        "Received message from: {}",
        received_message.username().unwrap()
      ),
    };
    write_message_to_stream(&mut writer, &response)
      .await
      .unwrap();

    received_message
  });

  let client_handle = tokio::spawn(async move {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    write_message_to_stream(&mut writer, &test_message)
      .await
      .unwrap();

    let mut buffer = vec![0u8; 4096];
    let response: ServerMessage = read_message_from_stream(&mut reader, &mut buffer)
      .await
      .unwrap();

    response
  });

  let (received_message, response) = tokio::join!(server_handle, client_handle);
  let received_message = received_message.unwrap();
  let response = response.unwrap();

  match received_message {
    ClientMessage::Message { username, content } => {
      assert_eq!(username, "testuser");
      assert_eq!(content, "Hello via TCP!");
    }
    _ => panic!("Expected ClientMessage::Message"),
  }

  match response {
    ServerMessage::Success { message } => {
      assert!(message.contains("testuser"));
    }
    _ => panic!("Expected ServerMessage::Success"),
  }
}

#[tokio::test]
async fn test_transport_layer_error_handling() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    drop(stream);
  });

  let client_handle = tokio::spawn(async move {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut reader, _) = stream.into_split();

    let mut buffer = vec![0u8; 4096];
    let result: Result<ClientMessage, ApplicationError> =
      read_message_from_stream(&mut reader, &mut buffer).await;

    result
  });

  server_handle.await.unwrap();
  let read_result = client_handle.await.unwrap();

  assert!(matches!(
    read_result,
    Err(ApplicationError::ClientReadStreamClosed)
  ));
}

#[tokio::test]
async fn test_large_message_handling() {
  let large_content = "x".repeat(50_000);
  let test_message = ClientMessage::Message {
    username: "largeuser".to_string(),
    content: large_content.clone(),
  };

  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, _) = stream.into_split();

    let mut buffer = vec![0u8; 65536];
    let received_message: ClientMessage = read_message_from_stream(&mut reader, &mut buffer)
      .await
      .unwrap();

    received_message
  });

  let client_handle = tokio::spawn(async move {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (_, mut writer) = stream.into_split();

    write_message_to_stream(&mut writer, &test_message)
      .await
      .unwrap();
  });

  let (received_message, _) = tokio::join!(server_handle, client_handle);

  match received_message.unwrap() {
    ClientMessage::Message { username, content } => {
      assert_eq!(username, "largeuser");
      assert_eq!(content.len(), 50_000);
      assert_eq!(content, large_content);
    }
    _ => panic!("Expected large ClientMessage::Message"),
  }
}

#[tokio::test]
async fn test_concurrent_message_transfer() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();

  let num_clients = 5;
  let messages_per_client = 3;

  let server_handle = tokio::spawn(async move {
    let mut expected_messages = Vec::new();

    for i in 0..num_clients {
      let (stream, _) = listener.accept().await.unwrap();
      let (mut reader, _) = stream.into_split();

      for j in 0..messages_per_client {
        let mut buffer = vec![0u8; 4096];
        let received_message: ClientMessage = read_message_from_stream(&mut reader, &mut buffer)
          .await
          .unwrap();

        expected_messages.push(format!("client{}_msg{}", i, j));

        match &received_message {
          ClientMessage::Message { username, content } => {
            assert_eq!(username, &format!("client{}", i));
            assert_eq!(content, &format!("message {}", j));
          }
          _ => panic!("Expected ClientMessage::Message"),
        }
      }
    }

    expected_messages
  });

  let mut client_handles = Vec::new();
  for i in 0..num_clients {
    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (_, mut writer) = stream.into_split();

      for j in 0..messages_per_client {
        let message = ClientMessage::Message {
          username: format!("client{}", i),
          content: format!("message {}", j),
        };

        write_message_to_stream(&mut writer, &message)
          .await
          .unwrap();
      }
    });
    client_handles.push(client_handle);
  }

  for handle in client_handles {
    handle.await.unwrap();
  }

  let received_messages = server_handle.await.unwrap();
  assert_eq!(received_messages.len(), num_clients * messages_per_client);
}
