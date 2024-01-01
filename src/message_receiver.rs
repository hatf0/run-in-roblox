use anyhow::Result;
use async_channel::RecvError;
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Json, State, Query},
    routing::{get, post},
    Router, http::StatusCode,
};
use axum_macros::debug_handler;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, sync::Arc, time::Duration, collections::HashMap};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};

#[derive(Debug, Clone)]
pub enum Message {
    Start { server: String },
    Stop { server: String },
    Messages(Vec<RobloxMessage>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RobloxMessage {
    Output { level: OutputLevel, body: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartMessage {
    server: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StopMessage {
    server: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusMessage {
    run: bool,
    src: String
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum OutputLevel {
    Print,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Svc {
    message_tx: async_channel::Sender<Message>,
    message_rx: async_channel::Receiver<Message>,
    shutdown_tx: async_channel::Sender<()>,
    shutdown_rx: async_channel::Receiver<()>,
    script_source: String,
}

impl Svc {
    async fn ping(State(svc): State<Arc<Svc>>) -> &'static str {
        "OK!"
    }

    async fn start_handler(
        State(svc): State<Arc<Svc>>,
        Json(start_message): Json<StartMessage>,
    ) -> Result<&'static str, StatusCode> {
        svc.message_tx
            .send(Message::Start {
                server: start_message.server.clone(),
            })
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok("Started")
    }

    async fn stop_handler(
        State(svc): State<Arc<Svc>>,
        Json(stop_message): Json<StopMessage>,
    ) -> Result<&'static str, StatusCode> {
        svc.message_tx
            .send(Message::Stop {
                server: stop_message.server.clone(),
            })
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok("Stopped")
    }

    async fn messages_handler(
        State(svc): State<Arc<Svc>>,
        Json(messages): Json<Vec<RobloxMessage>>
    ) -> Result<&'static str, StatusCode> {
        svc.message_tx
            .send(Message::Messages(messages.clone()))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok("Got it!")
    }

    async fn status_handler(
        State(svc): State<Arc<Svc>>,
        Query(params): Query<HashMap<String, String>>
    ) -> Result<Json<StatusMessage>, StatusCode> {
        Ok(Json(StatusMessage { 
            run: true,
            src: svc.script_source.clone(),
        }))
    }

    pub async fn start(script_src: String) -> Result<Arc<Self>> {
        let (message_tx, message_rx) = async_channel::bounded(100);
        let (shutdown_tx, shutdown_rx) = async_channel::bounded(1);

        let svc = Arc::new(Svc {
            message_tx,
            message_rx,
            shutdown_tx,
            shutdown_rx,
            script_source: script_src
        });

        let svc_clone = svc.clone();
        let shutdown_signal = async move { svc_clone.shutdown_rx.recv().await.unwrap() };

        let svc_clone = svc.clone();
        let app: Router = Router::new()
            .route("/ping", get(Self::ping))
            .route("/start", post(Self::start_handler))
            .route("/stop", post(Self::stop_handler))
            .route("/messages", post(Self::messages_handler))
            .route("/status", get(Self::status_handler))
            .with_state(svc_clone);

        let listener = TcpListener::bind("127.0.0.1:7777").await?;
        tokio::task::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .unwrap()
        });
        Ok(svc)
    }

    pub async fn recv(&self) -> Message {
        self.message_rx.recv().await.unwrap()
    }

    pub async fn recv_timeout(&self, timeout: Duration) -> Option<Result<Message, RecvError>> {
        match tokio::time::timeout(timeout, self.message_rx.recv()).await {
            Ok(inner) => Some(inner),
            Err(_) => None,
        }
    }

    pub async fn stop(&self) {
        let _dont_care = self.shutdown_tx.send(()).await.unwrap();
    }
}
