//! Per-client budget for authenticated upstream calls.
//!
//! Charged where teapot spends X API quota, so cache hits, media proxying and
//! static assets are free without being listed anywhere. Every tier must admit
//! the call, so a crawler settles onto the slowest refill while a reader stays
//! in the fastest.

use std::{
   collections::HashMap,
   future::Future,
   net::IpAddr,
   sync::Arc,
   time::{
      Duration,
      Instant,
   },
};

use axum::http::HeaderMap;
use tokio::sync::Mutex;

#[derive(Clone, Copy)]
pub struct Tier {
   pub refill_per_sec: f64,
   pub capacity:       f64,
}

/// Widening horizons. The tightest tier decides.
const TIERS: [Tier; 3] = [
   Tier {
      refill_per_sec: 1.0,
      capacity:       20.0,
   },
   Tier {
      refill_per_sec: 0.2,
      capacity:       60.0,
   },
   Tier {
      refill_per_sec: 0.05,
      capacity:       200.0,
   },
];

const IDLE_EVICTION: Duration = Duration::from_secs(600);

/// Reaching this evicts the coldest entries, so a caller cycling through
/// addresses cannot turn the map itself into a memory attack.
const MAX_CLIENTS: usize = 0x8000;

/// Sweeping is O(n), so a timer rather than a per-insert threshold, which a
/// flood of new keys would make quadratic.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

struct Buckets {
   tokens: [f64; TIERS.len()],
   seen:   Instant,
}

impl Buckets {
   fn new() -> Self {
      Self {
         tokens: [TIERS[0].capacity, TIERS[1].capacity, TIERS[2].capacity],
         seen:   Instant::now(),
      }
   }

   fn refund(&mut self, cost: f64) {
      for (tokens, tier) in self.tokens.iter_mut().zip(TIERS) {
         *tokens = (*tokens + cost).min(tier.capacity);
      }
   }

   fn try_spend(&mut self, cost: f64, now: Instant) -> bool {
      let elapsed = now.duration_since(self.seen).as_secs_f64();
      self.seen = now;

      for (tokens, tier) in self.tokens.iter_mut().zip(TIERS) {
         *tokens = tier
            .refill_per_sec
            .mul_add(elapsed, *tokens)
            .min(tier.capacity);
      }

      if self.tokens.iter().any(|tokens| *tokens < cost) {
         return false;
      }
      for tokens in &mut self.tokens {
         *tokens -= cost;
      }

      true
   }
}

#[derive(Clone)]
#[expect(
   clippy::module_name_repetitions,
   reason = "ClientBudget reads better than Budget at its call sites"
)]
pub struct ClientBudget {
   clients: Arc<Mutex<Clients>>,
   enabled: bool,
}

struct Clients {
   buckets:    HashMap<ClientKey, Buckets>,
   last_sweep: Instant,
}

impl Clients {
   fn evict(&mut self, now: Instant) {
      if now.duration_since(self.last_sweep) < SWEEP_INTERVAL && self.buckets.len() < MAX_CLIENTS {
         return;
      }
      self.last_sweep = now;
      self
         .buckets
         .retain(|_, buckets| now.duration_since(buckets.seen) < IDLE_EVICTION);

      if self.buckets.len() <= MAX_CLIENTS {
         return;
      }
      let mut seen = self
         .buckets
         .iter()
         .map(|(key, buckets)| (buckets.seen, key.clone()))
         .collect::<Vec<_>>();
      seen.sort_unstable_by_key(|&(seen, _)| seen);
      for (_, key) in seen
         .into_iter()
         .take(self.buckets.len() - MAX_CLIENTS * 3 / 4)
      {
         self.buckets.remove(&key);
      }
   }
}

/// A caller, narrowed to the IPv6 /64 so that rotating within one allocation
/// does not hand out a fresh budget per request.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ClientKey(String);

impl ClientKey {
   #[must_use]
   pub fn from_ip(ip: IpAddr) -> Self {
      match ip {
         IpAddr::V4(v4) => Self(v4.to_string()),
         IpAddr::V6(v6) => {
            let [one, two, three, four, ..] = v6.segments();
            Self(format!("{one:x}:{two:x}:{three:x}:{four:x}::/64"))
         },
      }
   }
}

tokio::task_local! {
   /// The caller of the request being served, if a trusted proxy named one.
   pub static CLIENT: ClientKey;
}

/// A spawned task starts with an empty task-local scope, so anything reaching
/// the API from `tokio::spawn` or a `JoinSet` must wrap its future in this or
/// those calls go unbilled.
pub fn scoped<F>(fut: F) -> impl Future<Output = F::Output>
where
   F: Future,
{
   let client = current_client();
   async move {
      match client {
         Some(client) => CLIENT.scope(client, fut).await,
         None => fut.await,
      }
   }
}

#[must_use]
pub fn current_client() -> Option<ClientKey> {
   CLIENT.try_with(Clone::clone).ok()
}

/// The socket peer is the only address a caller cannot choose, so it wins
/// unless the peer is a configured proxy.
///
/// `X-Forwarded-For` is read right to left because nginx's
/// `$proxy_add_x_forwarded_for` appends rather than replaces, leaving the
/// leftmost entry attacker-controlled.
#[must_use]
pub fn client_from(peer: IpAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> ClientKey {
   if !trusted.contains(&peer) {
      return ClientKey::from_ip(peer);
   }

   let forwarded = headers
      .get("x-forwarded-for")
      .and_then(|value| value.to_str().ok())
      .into_iter()
      .flat_map(|value| value.rsplit(','))
      .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
      .find(|address| !trusted.contains(address));

   ClientKey::from_ip(forwarded.unwrap_or(peer))
}

impl ClientBudget {
   #[must_use]
   pub fn new(enabled: bool) -> Self {
      Self {
         clients: Arc::new(Mutex::new(Clients {
            buckets:    HashMap::new(),
            last_sweep: Instant::now(),
         })),
         enabled,
      }
   }

   /// Charging happens before a session is acquired, so every path that fails
   /// between the two has to come back here or the caller pays for nothing.
   pub async fn refund(&self, client: &ClientKey, cost: f64) {
      if !self.enabled {
         return;
      }

      let mut clients = self.clients.lock().await;
      if let Some(buckets) = clients.buckets.get_mut(client) {
         buckets.refund(cost);
      }
   }

   pub async fn try_spend(&self, client: &ClientKey, cost: f64) -> bool {
      if !self.enabled {
         return true;
      }

      let now = Instant::now();
      let mut clients = self.clients.lock().await;

      if let Some(buckets) = clients.buckets.get_mut(client) {
         return buckets.try_spend(cost, now);
      }

      clients.evict(now);
      clients
         .buckets
         .entry(client.clone())
         .or_insert_with(Buckets::new)
         .try_spend(cost, now)
   }
}
