use mt_net::{ReceiverExt, SenderExt, ToCltPkt, ToSrvPkt};

#[tokio::main]
async fn main() {
    let (tx, mut rx, worker) = mt_net::connect(&std::env::args().nth(1).expect("missing argument"))
        .await
        .unwrap();

    let mut auth = mt_auth::Auth::new(tx.clone(), "texmodbot", "owo", "en_US");
    let worker = tokio::spawn(worker.run());

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
            _ = tokio::signal::ctrl_c() => {
                tx.close();
            }
        }
    }

    worker.await.unwrap();
}
