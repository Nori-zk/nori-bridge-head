use async_trait::async_trait;
use anyhow::{Result, Context};
use futures::future::join_all;

#[async_trait]
pub trait EventListener<T: Clone>: Send + Sync {
    async fn on_event(&mut self, data: T) -> Result<()>;
}

// Delete below

pub struct EventDispatcher<T: Clone> {
    listeners: Vec<Box<dyn EventListener<T> + Send>>
}

impl<T: std::clone::Clone> EventDispatcher<T> {
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
        }
    }

    /*pub fn add_listener<L>(&mut self, listener: L)
    where
        L: EventListener<T> + 'static + Send,
    {
        self.listeners.push(Box::new(listener));
    }*/
    pub fn add_listener(&mut self, listener: Box<dyn EventListener<T> + Send>) {
        self.listeners.push(listener);
    }


    pub async fn trigger(&mut self, data: T) -> Result<()> {
        let _futures: Vec<_> = self.listeners.iter_mut().map(|listener| {
            listener.on_event(data.clone())
        }).collect();

        let result = join_all(_futures).await;

        for res in result {
            res.context("Failed to process listener")?; // Propagate the error if any listener failed
        }

        Ok(())
    }
}

