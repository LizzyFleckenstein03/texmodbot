use clap::Parser;
use enumset::{EnumSet, EnumSetType};
use futures_util::future::OptionFuture;
use mt_auth::Auth;
use mt_net::{CltSender, ReceiverExt, SenderExt, ToCltPkt, ToSrvPkt};
use std::collections::HashSet;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{sleep, Sleep};

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Args {
    /// Server address. Format: address:port
    #[clap(value_parser)]
    address: String,

    /// Quit after QUIT_AFTER seconds
    #[clap(short, long, value_parser)]
    quit_after_seconds: Option<f32>,

    /// Quit after having received item and node definitions
    #[clap(short = 'Q', long, value_parser, default_value_t = false)]
    quit_after_defs: bool,

    /// Player name
    #[clap(short, long, value_parser, default_value = "texmodbot")]
    username: String,

    /// Password
    #[clap(short, long, value_parser, default_value = "owo")]
    password: String,
}

#[derive(EnumSetType)]
enum DefType {
    NodeDef,
    ItemDef,
}

struct Bot {
    conn: CltSender,
    quit_after_defs: bool,
    auth: Auth,
    pending: EnumSet<DefType>,
    has: HashSet<String>,
}

impl Bot {
    fn got_def(&mut self, def: DefType) {
        self.pending.remove(def);
        if self.quit_after_defs && self.pending.is_empty() {
            self.conn.close()
        }
    }

    async fn handle_pkt(&mut self, pkt: ToCltPkt) {
        use ToCltPkt::*;

        self.auth.handle_pkt(&pkt).await;

        let mut print_texture = |tex: &String| {
            if !tex.is_empty() && self.has.insert(tex.clone()) {
                println!("{tex}");
            }
        };

        match pkt {
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
                    .for_each(print_texture);

                self.got_def(DefType::NodeDef);
            }
            ItemDefs { defs, .. } => {
                defs.iter()
                    .flat_map(|def| {
                        [
                            &def.inventory_image,
                            &def.wield_image,
                            &def.inventory_overlay,
                            &def.wield_overlay,
                        ]
                    })
                    .for_each(print_texture);

                self.got_def(DefType::ItemDef);
            }
            Kick(reason) => {
                eprintln!("kicked: {reason}");
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let Args {
        address,
        quit_after_seconds,
        quit_after_defs,
        username,
        password,
    } = Args::parse();

    let (tx, mut rx, worker) = mt_net::connect(&address).await.unwrap();

    let mut bot = Bot {
        auth: Auth::new(tx.clone(), username, password, "en_US"),
        conn: tx,
        quit_after_defs,
        pending: EnumSet::all(),
        has: HashSet::new(),
    };

    let worker = tokio::spawn(worker.run());

    let mut quit_sleep: Option<Pin<Box<Sleep>>> = quit_after_seconds.and_then(|x| {
        if x >= 0.0 {
            Some(Box::pin(sleep(Duration::from_secs_f32(x))))
        } else {
            None
        }
    });

    loop {
        tokio::select! {
            pkt = rx.recv() => match pkt {
                None => break,
                Some(Err(e)) => eprintln!("{e}"),
                Some(Ok(pkt)) => bot.handle_pkt(pkt).await,
            },
            _ = bot.auth.poll() => {
                bot.conn
                    .send(&ToSrvPkt::CltReady {
                        major: 0,
                        minor: 0,
                        patch: 0,
                        reserved: 0,
                        version: "https://github.com/LizzyFleckenstein03/texmodbot".into(),
                        formspec: 4,
                    })
                    .await
                    .unwrap();
            }
            Some(_) = OptionFuture::from(quit_sleep.as_mut()) => {
                bot.conn.close();
            }
            _ = tokio::signal::ctrl_c() => {
                bot.conn.close();
            }
        }
    }

    worker.await.unwrap();
}
