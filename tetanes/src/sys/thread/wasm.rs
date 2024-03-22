use std::future::Future;

/// Spawn a future to be run until completion.
pub fn spawn_impl<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}
