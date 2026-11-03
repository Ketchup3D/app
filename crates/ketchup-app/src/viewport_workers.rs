//! Reuse a bounded number of native workers across frames. Shadow preparation
//! has its own pool so a background preview cannot queue ahead of drawing.
use rayon::prelude::*;
use std::sync::OnceLock;

pub(crate) enum Kind {
    Render,
    Shadows,
}

pub(crate) fn parallelism() -> usize {
    static COUNT: OnceLock<usize> = OnceLock::new();
    *COUNT.get_or_init(|| {
        std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4)
    })
}

pub(crate) fn map<T: Send, U: Send>(
    kind: Kind,
    items: Vec<T>,
    apply: impl Fn(T) -> U + Send + Sync,
) -> Vec<U> {
    static RENDER: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    static SHADOWS: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    let (pool, name) = match kind {
        Kind::Render => (&RENDER, "ketchup-render"),
        Kind::Shadows => (&SHADOWS, "ketchup-shadow"),
    };
    let pool = pool.get_or_init(|| {
        if parallelism() <= 1 {
            return None;
        }
        rayon::ThreadPoolBuilder::new()
            .num_threads(parallelism())
            .thread_name(move |i| format!("{name}-{i}"))
            .build()
            .ok()
    });
    if let Some(pool) = pool {
        pool.install(|| items.into_par_iter().map(apply).collect())
    } else {
        items.into_iter().map(apply).collect()
    }
}
