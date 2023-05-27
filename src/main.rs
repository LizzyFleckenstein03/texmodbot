use futures::future::OptionFuture;
use mt_net::{CltSender, ReceiverExt, SenderExt, ToCltPkt, ToSrvPkt};
use rand::RngCore;
use sha2::Sha256;
use srp::{client::SrpClient, groups::G_2048};
use std::{future::Future, time::Duration};
use tokio::time::{interval, Instant, Interval};

enum AuthState {
    Init(Interval),
    Verify(Vec<u8>, SrpClient<'static, Sha256>),
    Done,
}

struct Conn {
    tx: CltSender,
    auth: AuthState,
    username: String,
    password: String,
}

fn maybe_tick(iv: Option<&mut Interval>) -> OptionFuture<impl Future<Output = Instant> + '_> {
    OptionFuture::from(iv.map(Interval::tick))
}

#[tokio::main]
async fn main() {
    let (tx, mut rx, worker) = mt_net::connect(&std::env::args().nth(1).expect("missing argument"))
        .await
        .unwrap();

    let mut conn = Conn {
        tx,
        auth: AuthState::Init(interval(Duration::from_millis(100))),
        username: "texmodbot".into(),
        password: "owo".into(),
    };

    let init_pkt = ToSrvPkt::Init {
        serialize_version: 29,
        proto_version: 40..=40,
        player_name: conn.username.clone(),
        send_full_item_meta: false,
    };

    let worker_thread = tokio::spawn(worker.run());

    loop {
        tokio::select! {
            pkt = rx.recv() => match pkt {
                None => break,
                Some(Err(e)) => eprintln!("{e}"),
                Some(Ok(v)) => conn.handle_pkt(v).await,
            },
            Some(_) = maybe_tick(match &mut conn.auth {
                AuthState::Init(iv) => Some(iv),
                _ => None,
            }) => {
                conn.tx.send(&init_pkt).await.unwrap();
            }
            _ = tokio::signal::ctrl_c() => {
                conn.tx.close();
            }
        }
    }

    worker_thread.await.unwrap();
}

impl Conn {
    async fn handle_pkt(&mut self, pkt: ToCltPkt) {
        use ToCltPkt::*;

        match pkt {
            Hello {
                auth_methods,
                username: name,
                ..
            } => {
                use mt_net::AuthMethod;

                if !matches!(self.auth, AuthState::Init(_)) {
                    return;
                }

                let srp = SrpClient::<Sha256>::new(&G_2048);

                let mut rand_bytes = vec![0; 32];
                rand::thread_rng().fill_bytes(&mut rand_bytes);

                if self.username != name {
                    panic!("username changed");
                }

                if auth_methods.contains(AuthMethod::FirstSrp) {
                    let verifier = srp.compute_verifier(
                        self.username.to_lowercase().as_bytes(),
                        self.password.as_bytes(),
                        &rand_bytes,
                    );

                    self.tx
                        .send(&ToSrvPkt::FirstSrp {
                            salt: rand_bytes,
                            verifier,
                            empty_passwd: self.password.is_empty(),
                        })
                        .await
                        .unwrap();

                    self.auth = AuthState::Done;
                } else if auth_methods.contains(AuthMethod::Srp) {
                    let a = srp.compute_public_ephemeral(&rand_bytes);

                    self.tx
                        .send(&ToSrvPkt::SrpBytesA { a, no_sha1: true })
                        .await
                        .unwrap();

                    self.auth = AuthState::Verify(rand_bytes, srp);
                } else {
                    panic!("unsupported auth methods: {auth_methods:?}");
                }
            }
            SrpBytesSaltB { salt, b } => {
                if let AuthState::Verify(a, srp) = &self.auth {
                    let m = srp
                        .process_reply(
                            a,
                            self.username.to_lowercase().as_bytes(),
                            self.password.as_bytes(),
                            &salt,
                            &b,
                        )
                        .unwrap()
                        .proof()
                        .into();

                    self.tx.send(&ToSrvPkt::SrpBytesM { m }).await.unwrap();

                    self.auth = AuthState::Done;
                }
            }
            NodeDefs(defs) => {
                defs.0
                    .values()
                    .flat_map(|def| {
                        std::iter::empty()
                            .chain(&def.tiles)
                            .chain(&def.special_tiles)
                            .chain(&def.overlay_tiles)
                    })
                    .map(|tile| &tile.texture.name)
                    .for_each(|texture| {
                        if !texture.is_empty() {
                            println!("{texture}");
                        }
                    });
            }
            Kick(reason) => {
                eprintln!("kicked: {reason}");
            }
            AcceptAuth { .. } => {
                self.tx
                    .send(&ToSrvPkt::Init2 {
                        lang: "en_US".into(),
                    })
                    .await
                    .unwrap();
            }
            _ => {}
        }
    }
}
