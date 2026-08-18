//! Two servers over one fixture, for `scripts/snap.sh` — a CLI-shaped one and
//! an app-shaped one (app_ui, Places, Recent, one greyed entry), so a snapshot
//! covers both kinds of page a refactor could disturb.
//!
//! Prints `PORTS <cli> <app>` and then sleeps; the script does the fetching.

use std::path::PathBuf;
use treeserve::{Config, RootStatus};

fn main() {
    let fx = PathBuf::from(std::env::args().nth(1).expect("usage: snapshot_server <fixture-dir>"))
        .canonicalize()
        .unwrap();

    let mut c1 = Config::new(fx.clone());
    c1.port = 0;
    c1.threads = 2;
    let s1 = treeserve::spawn(c1).unwrap();

    let mut c2 = Config::new(fx.clone());
    c2.port = 0;
    c2.threads = 2;
    c2.app_ui = true;
    c2.places = vec![
        ("Home".to_string(), fx.join("sub").display().to_string()),
        ("Gone".to_string(), fx.join("gone").display().to_string()),
    ];
    let s2 = treeserve::spawn(c2).unwrap();
    s2.state.cfg.set_recent(vec![
        fx.join("sub").display().to_string(),
        fx.display().to_string(),
    ]);
    s2.state
        .cfg
        .set_root_status(fx.join("gone").display().to_string(), RootStatus::Missing);

    println!("PORTS {} {}", s1.addr.port(), s2.addr.port());
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
