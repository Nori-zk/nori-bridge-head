use tokio::time::{timeout, Duration};
use anyhow::{anyhow, Result};
use futures::{
    future::{select_ok, BoxFuture},
};
use log::warn;

pub mod execution;
pub mod consensus;

/// Takes a closure that produces a future for each provided client, tries the principle (the primary) one first 
/// and then falls back to the remaining providers and returns the result or error if all fail.
pub async fn query_with_fallback<F, P, R>(
    principal_provider: &P,
    backup_providers: &[P],
    f: F,
    timeout_duration: Duration,
) -> Result<R>
where
    F: Fn(P) -> BoxFuture<'static, Result<R>>,
    P: Clone,
    R: 'static,
{
    match timeout(timeout_duration, f(principal_provider.clone())).await {
        Ok(Ok(result)) => return Ok(result),
        Ok(Err(err)) => {
            warn!("Principal provider failed: {}.", err);
        }
        Err(_) => {
            warn!("Principal provider timed out after {:?}", timeout_duration);
        }
    }

    if backup_providers.is_empty() {
        return Err(anyhow!(
            "Principal provider failed and no backups available."
        ));
    }

    warn!("Trying backup providers...");

    let backup_futures: Vec<BoxFuture<'static, Result<R>>> = backup_providers
        .iter()
        .map(|provider| {
            let fut = f(provider.clone());
            Box::pin(async move {
                timeout(timeout_duration, fut)
                    .await
                    .map_err(|_| anyhow!("Provider timed out after {:?}", timeout_duration))?
            }) as BoxFuture<'static, Result<R>>
        })
        .collect();

    match select_ok(backup_futures).await {
        Ok((result, _)) => Ok(result),
        Err(e) => Err(anyhow!("All backups failed: {}", e)),
    }
}

// https://github.com/across-protocol/sp1-helios
/// Takes a closure that produces a future for each provided client, wraps each with a timeout,
/// and returns the result of the first successfully completed future.
/// Returns an error if all fail or time out.
pub async fn multiplex<F, P, R>(
    f: F,
    providers: &[P],
    timeout_duration: Duration,
) -> Result<R>
where
    F: Fn(P) -> BoxFuture<'static, Result<R>>,
    P: Clone,
    R: 'static,
{
    if providers.is_empty() {
        return Err(anyhow!("No providers available."));
    }

    let futs = providers.iter().map(|client| {
        let fut = f(client.clone());
        Box::pin(async move {
            timeout(timeout_duration, fut)
                .await
                .map_err(|_| anyhow!("Provider timed out after {:?}", timeout_duration))?
        }) as BoxFuture<'static, Result<R>>
    });

    match select_ok(futs).await {
        Ok((result, _)) => Ok(result),
        Err(e) => Err(anyhow!("All multiplexed requests failed: {}", e)),
    }
}