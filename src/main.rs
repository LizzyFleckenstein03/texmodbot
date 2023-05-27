use clap::Parser;
use futures_util::future::OptionFuture;
use mt_net::{ReceiverExt, SenderExt, ToCltPkt, ToSrvPkt};
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

    /// Quit after having received node definitions
    #[clap(short = 'Q', long, value_parser, default_value_t = false)]
    quit_after_nodes: bool,

    /// Player name
    #[clap(short, long, value_parser, default_value = "texmodbot")]
    username: String,

    /// Password
    #[clap(short, long, value_parser, default_value = "owo")]
    password: String,
}

#[tokio::main]
async fn main() {
    let Args {
        address,
        quit_after_seconds,
        quit_after_nodes,
        username,
        password,
    } = Args::parse();

    let (tx, mut rx, worker) = mt_net::connect(&address).await.unwrap();

    let mut auth = mt_auth::Auth::new(tx.clone(), username, password, "en_US");
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
                Some(Ok(pkt)) => {
                    use ToCltPkt::*;

                    auth.handle_pkt(&pkt).await;

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
                                .for_each(|texture| {
                                    if !texture.is_empty() {
                                        println!("{texture}");
                                    }
                                });

                            if quit_after_nodes {
                                tx.close();
                            }
                        }
                        ItemDefs{defs, aliases: _} => {
                            defs.into_iter()
                                .for_each(|itemdef| {
                                    [itemdef.inventory_image, itemdef.wield_image, itemdef.inventory_overlay, itemdef.wield_overlay]
                                        .into_iter()
                                        .for_each(|texture|
                                            if !texture.is_empty() {
                                                println!("{texture}");
                                            }
                                        );
                                });
                        }
                        Kick(reason) => {
                            eprintln!("kicked: {reason}");
                        }
                        _ => {}
                    }
                }
            },
            _ = auth.poll() => {
                tx
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
            },
            Some(_) = OptionFuture::from(quit_sleep.as_mut()) => {
                tx.close();
            }
            _ = tokio::signal::ctrl_c() => {
                tx.close();
            }
        }
    }

    worker.await.unwrap();
}
