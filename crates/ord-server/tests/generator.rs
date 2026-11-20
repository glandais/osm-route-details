//! Le générateur servi par le service (lot 3a, note §11) : graphe, vues et surcouches **synthétiques**.
//!
//! Une propriété d'isolation par test : un environnement ne peut ni lire, ni utiliser, ni écraser, ni effacer, ni
//! détecter la surcouche d'un autre ; aucun identifiant ne sort du dossier de stockage ; un dépôt raté ou interrompu
//! laisse la surcouche précédente en service.

mod common;

use std::io::Write;
use std::time::{Duration, SystemTime};

use axum::http::StatusCode;
use axum::Router;
use common::generator::*;
use common::*;
use ord_server::config::Config;
use ord_server::{router, AppState};
use serde_json::{json, Value};

struct World {
    dir: tempfile::TempDir,
    config: Config,
    state: AppState,
    app: Router,
}

impl World {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let files = write_files(dir.path(), "g1", 21);
        let mut config = config_with(test_config(), dir.path(), &files);
        tweak(&mut config);
        let state = AppState::new(&config).expect("état");
        let app = router(state.clone());
        World { dir, config, state, app }
    }

    async fn send(&self, req: axum::http::Request<axum::body::Body>) -> Reply {
        send(self.app.clone(), req).await
    }

    fn store_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("overlays")
    }

    async fn deposit(&self, key: &str, team: &str, digest: &str) -> Reply {
        self.send(put_digest(key, team, digest, team_deposit())).await
    }

    /// Les fichiers de surcouche du stockage (`.ordo.enc`) : les profils de goût (`.ordp.enc`, 3b) sont comptés à part.
    fn overlays(&self) -> Vec<String> {
        tree(&self.store_dir()).into_iter().filter(|f| f.ends_with(".ordo.enc")).collect()
    }

    /// Les fichiers de profil de goût du stockage.
    fn profiles(&self) -> Vec<String> {
        tree(&self.store_dir()).into_iter().filter(|f| f.ends_with(".ordp.enc")).collect()
    }

    async fn state_of(&self, key: &str, team: &str) -> Value {
        let r = self.send(status(key, team)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        r.json()
    }
}

// --- Cloisonnement -------------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_environment_cannot_read_use_overwrite_delete_or_detect_anothers_overlay() {
    let w = World::new();
    let (k1, k2) = (key(1), key(2));
    let r = w.deposit(&k1, "team-a", "d1").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(w.state_of(&k1, "team-a").await["state"], "ready");

    // Lire : la même référence d'équipe sous une autre clé est absente, sans le moindre indice.
    let seen = w.state_of(&k2, "team-a").await;
    assert_eq!(seen["state"], "absent");
    assert!(seen.get("deposited_at").is_none() && seen.get("source_digest").is_none(), "{seen}");

    // Utiliser : 409 au corps identique (hors `instance`) à celui d'une équipe inconnue sous la clé 1.
    let body = generate_body(0.5);
    let other_env = w.send(generate(Some(&k2), Some("team-a"), &body)).await;
    let unknown_team = w.send(generate(Some(&k1), Some("team-unknown"), &body)).await;
    assert_problem(&other_env, StatusCode::CONFLICT, "overlay_required");
    assert_eq!(without_instance(&other_env.text), without_instance(&unknown_team.text));
    assert_eq!(other_env.json()["details"]["state"], "absent");
    // … et la même demande sous la bonne clé marche.
    let ok = w.send(generate(Some(&k1), Some("team-a"), &body)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);

    // Écraser : un dépôt sous la clé 2 pour la même référence n'altère pas la surcouche de la clé 1.
    let r = w.deposit(&k2, "team-a", "autre").await;
    assert_eq!(r.status, StatusCode::OK);
    let a = w.state_of(&k1, "team-a").await;
    assert_eq!((a["state"].as_str(), a["source_digest"].as_str()), (Some("ready"), Some("d1")));
    let b = w.state_of(&k2, "team-a").await;
    assert_eq!(b["source_digest"], "autre");

    // Effacer : sous la clé 3, 204 sans effet ; l'effacement de tout l'espace de la clé 3 non plus.
    assert_eq!(w.send(delete(&key(3), "team-a")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(w.send(delete_all(&key(3))).await.status, StatusCode::NO_CONTENT);
    assert_eq!(w.state_of(&k1, "team-a").await["state"], "ready");
    assert_eq!(w.state_of(&k2, "team-a").await["state"], "ready");

    // Détecter : 204 sous une autre clé comme pour une équipe qui n'a jamais existé ; chez soi, l'effacement agit.
    let ghost = w.send(delete(&k1, "never-deposited")).await;
    assert_eq!(ghost.status, StatusCode::NO_CONTENT);
    assert_eq!(ghost.text, "");
    assert_eq!(w.send(delete(&k1, "team-a")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(w.state_of(&k1, "team-a").await["state"], "absent");
    assert_eq!(w.state_of(&k2, "team-a").await["state"], "ready", "l'autre environnement n'a rien perdu");

    // Effacer tout l'espace de la clé 2.
    assert_eq!(w.send(delete_all(&k2)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(w.state_of(&k2, "team-a").await["state"], "absent");
}

#[tokio::test]
async fn identifiers_cannot_escape_the_storage_directory() {
    let w = World::new();
    let k = key(1);
    for bad in ["../x", "a/b", "a\\b", "..", ".", "a.b", "é", "", &"x".repeat(65), "a b"] {
        let r = w.send(put(&k, bad, team_deposit())).await;
        // Une valeur vide ou non ASCII peut être refusée avant même la route : l'essentiel est qu'elle n'aboutisse pas.
        assert!(r.status.is_client_error(), "{bad:?} → {}", r.status);
        assert_ne!(r.status, StatusCode::OK);
    }
    for bad_key in [&key(1)[..42], &format!("{}A", key(1)), "../../etc/passwd", ""] {
        let r = w.send(put(bad_key, "team", team_deposit())).await;
        assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
        assert!(!r.text.contains(bad_key) || bad_key.is_empty(), "la valeur de l'en-tête ne revient pas");
    }
    // Un dépôt valide ne crée que des noms hexadécimaux : `graph-<hex>/<32 hex>/<64 hex>.ordo.enc`.
    assert_eq!(w.deposit(&k, "team_1-OK", "d").await.status, StatusCode::OK);
    let files = w.overlays();
    assert_eq!(files.len(), 1, "{files:?}");
    // Le profil de goût (3b) porte le même nom hexadécimal, avec son extension : rien d'autre n'est créé.
    let profiles = w.profiles();
    assert_eq!(profiles.len(), 1, "{profiles:?}");
    assert_eq!(profiles[0].trim_end_matches(".ordp.enc"), files[0].trim_end_matches(".ordo.enc"));
    assert_eq!(tree(&w.store_dir()).len(), 2);
    let parts: Vec<&str> = files[0].split('/').collect();
    assert_eq!(parts.len(), 3);
    let hex = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit());
    assert!(parts[0].starts_with("graph-") && hex(&parts[0][6..]), "{files:?}");
    assert!(hex(parts[1]) && parts[1].len() == 32, "{files:?}");
    assert!(parts[2].ends_with(".ordo.enc") && hex(&parts[2][..parts[2].len() - 9]), "{files:?}");
    assert!(!files[0].contains("team_1-OK"));
}

// --- Atomicité ------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_failed_or_interrupted_deposit_leaves_the_previous_overlay_serving() {
    let w = World::with(|c| {
        c.generator.deposit.max_document_bytes = 4 * 1024 * 1024;
        c.generator.deposit.max_tracks = 4;
    });
    let (k, team) = (key(1), "team-a");
    assert_eq!(w.deposit(&k, team, "d1").await.status, StatusCode::OK);
    let before = w.state_of(&k, team).await;
    let files_before = tree(&w.store_dir());
    let doc = document(21, &[6, 8, 10, 12, 14], &[6, 8, 10, 12, 14]);
    let line = track_line("ROAD", false, &doc);

    let assert_untouched = |what: &str| {
        let (k, w) = (&k, &w);
        let (what, before, files_before) = (what.to_owned(), before.clone(), files_before.clone());
        async move {
            assert_eq!(w.state_of(k, team).await, before, "{what} : état");
            assert_eq!(tree(&w.store_dir()), files_before, "{what} : fichiers (aucun temporaire)");
            let r = w.send(generate(Some(k), Some(team), &generate_body(0.5))).await;
            assert_eq!(r.status, StatusCode::OK, "{what} : génération {}", r.text);
        }
    };

    // Sans ligne de fin.
    let r = w.send(put_digest(&k, team, "d2", ndjson(std::slice::from_ref(&line), None))).await;
    let body = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
    assert!(body["details"]["errors"][0]["pointer"].as_str().unwrap().starts_with("/lines/"));
    assert_untouched("sans fin").await;
    // Compte faux.
    let r = w.send(put_digest(&k, team, "d2", ndjson(std::slice::from_ref(&line), Some(7)))).await;
    assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
    assert_untouched("compte faux").await;
    // Ligne illisible, puis clé inconnue sur la ligne.
    for bad in ["{pas du json", r#"{"type":"ROAD","inconnue":1}"#] {
        let r = w.send(put_digest(&k, team, "d2", ndjson(&[bad.to_owned()], Some(1)))).await;
        let body = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(body["details"]["errors"][0]["pointer"], "/lines/1");
        assert!(!r.text.contains("inconnue"), "la valeur reçue ne revient pas");
    }
    assert_untouched("ligne illisible").await;
    // Données après la ligne de fin.
    let after = format!("{}{}\n", ndjson(std::slice::from_ref(&line), Some(1)), line);
    assert_problem(
        &w.send(put_digest(&k, team, "d2", after)).await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
    );
    assert_untouched("après la fin").await;
    // Erreur de flux au milieu du dépôt.
    let half = ndjson(std::slice::from_ref(&line), None).into_bytes();
    let broken = Frames::new(vec![half.clone(), half[..half.len() / 2].to_vec()]).then_error().body();
    let r = w.send(put_digest(&k, team, "d2", broken)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert_untouched("erreur de flux").await;
    // Silence : le délai sans octet est dépassé (504), rien n'est écrit.
    let stalled = Frames::new(vec![half.clone()]).then_stall().body();
    let r = w.send(put_digest(&k, team, "d2", stalled)).await;
    assert_problem(&r, StatusCode::GATEWAY_TIMEOUT, "timeout");
    assert_untouched("silence").await;
    // Trop de traces.
    let many = ndjson(&vec![line.clone(); 5], Some(5));
    let r = w.send(put_digest(&k, team, "d2", many)).await;
    let body = assert_problem(&r, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(body["details"]["limit"], "tracks");
    assert_untouched("trop de traces").await;
    // Disque plein simulé : l'écriture échoue après le fichier temporaire.
    let hooks = w.state.generator().hooks().expect("générateur prêt");
    hooks.fail_commit.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = w.send(put_digest(&k, team, "d2", team_deposit())).await;
    assert_problem(&r, StatusCode::INTERNAL_SERVER_ERROR, "internal");
    hooks.fail_commit.store(false, std::sync::atomic::Ordering::Relaxed);
    // Le profil de goût (3b) s'écrit **avant** la surcouche : le dépôt échoue, l'ancienne surcouche sert, et le profil
    // neuf (appris sur les traces du dépôt raté) l'accompagne ; seule la date de son apprentissage change.
    let mut after = w.state_of(&k, team).await;
    assert_eq!(after["source_digest"], "d1");
    after["profile"]["learned_at"] = before["profile"]["learned_at"].clone();
    assert_eq!(after, before, "disque plein : état");
    assert_eq!(tree(&w.store_dir()), files_before, "disque plein : fichiers (aucun temporaire)");
    let r = w.send(generate(Some(&k), Some(team), &generate_body(0.5))).await;
    assert_eq!(r.status, StatusCode::OK, "disque plein : génération {}", r.text);
    // Après la purge, toujours aucun temporaire ni fichier en trop.
    w.state.generator().purge_at(SystemTime::now()).unwrap();
    assert_eq!(tree(&w.store_dir()), files_before);

    // Un dépôt réussi remplace la surcouche (et change l'empreinte source).
    assert_eq!(w.deposit(&k, team, "d3").await.status, StatusCode::OK);
    assert_eq!(w.state_of(&k, team).await["source_digest"], "d3");
}

#[tokio::test]
async fn two_deposits_of_the_same_team_conflict_and_only_one_deposit_runs_at_a_time() {
    let w = World::with(|c| c.generator.deposit.idle_timeout_s = 30);
    let k = key(1);
    let doc = document(21, &[8], &[]);
    let first_line = ndjson(&[track_line("ROAD", false, &doc)], None).into_bytes();
    // Un dépôt qui s'arrête, sans fin : il tient le verrou de l'équipe et la place de dépôt.
    let app = w.app.clone();
    let req = put(&k, "team-a", Frames::new(vec![first_line]).then_stall().body());
    let held = tokio::spawn(async move { send(app, req).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let same = w.send(put(&k, "team-a", team_deposit())).await;
    assert_problem(&same, StatusCode::CONFLICT, "deposit_in_progress");
    // Même environnement, autre équipe : une seule place par environnement, refus immédiat.
    let other = w.send(put(&k, "team-b", team_deposit())).await;
    assert_problem(&other, StatusCode::SERVICE_UNAVAILABLE, "overloaded");
    assert!(other.headers.get("retry-after").is_some());
    // Effacer pendant le dépôt : refusé.
    assert_problem(&w.send(delete(&k, "team-a")).await, StatusCode::CONFLICT, "deposit_in_progress");
    assert_problem(&w.send(delete_all(&k)).await, StatusCode::CONFLICT, "deposit_in_progress");
    held.abort();
    let _ = held.await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Le client est parti : le verrou est rendu.
    assert_eq!(w.send(put(&k, "team-a", team_deposit())).await.status, StatusCode::OK);
}

// --- Persistance, graphe ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn overlays_survive_a_restart_and_a_new_graph_makes_them_stale_then_purges_them() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", "d1").await.status, StatusCode::OK);
    let graph_id = w.state_of(&k, "team-a").await["graph_id"].as_str().unwrap().to_owned();

    // Redémarrage : un nouvel état sur le même dossier.
    let again = router(AppState::new(&w.config).unwrap());
    let r = send(again.clone(), status(&k, "team-a")).await;
    assert_eq!(r.json()["state"], "ready");
    assert_eq!(r.json()["source_digest"], "d1");
    assert_eq!(
        send(again, generate(Some(&k), Some("team-a"), &generate_body(0.4))).await.status,
        StatusCode::OK
    );

    // Un autre graphe (20 × 20) : mêmes dossiers de stockage, autre identité.
    let files2 = write_files(w.dir.path(), "g2", 20);
    let config2 = config_with(test_config(), w.dir.path(), &files2);
    let state2 = AppState::new(&config2).unwrap();
    let app2 = router(state2.clone());
    let s = send(app2.clone(), status(&k, "team-a")).await.json();
    assert_eq!((s["state"].as_str(), s["stale_reason"].as_str()), (Some("stale"), Some("graph")));
    assert_ne!(s["graph_id"].as_str().unwrap(), graph_id);
    let r = send(app2.clone(), generate(Some(&k), Some("team-a"), &generate_body(0.4))).await;
    assert_problem(&r, StatusCode::CONFLICT, "overlay_required");
    assert_eq!(r.json()["details"]["state"], "stale");
    // La découverte marche, sans surcouche.
    let r = send(app2.clone(), generate(None, None, &generate_body(1.0))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // Avant la grâce, l'ancien dossier reste ; après, il part.
    let hooked = state2.generator();
    let early = hooked.purge_at(SystemTime::now() + Duration::from_secs(3600)).unwrap();
    assert_eq!(early.stale_graph, 0);
    assert_eq!(tree(&w.store_dir()).len(), 2, "la surcouche et son profil");
    let late = hooked.purge_at(SystemTime::now() + Duration::from_secs(49 * 3600)).unwrap();
    assert_eq!(late.stale_graph, 1);
    assert!(tree(&w.store_dir()).is_empty(), "{:?}", tree(&w.store_dir()));
    let s = send(app2.clone(), status(&k, "team-a")).await.json();
    assert_eq!(s["state"], "absent");
    // Redéposer sous le nouveau graphe : prêt.
    assert_eq!(send(app2.clone(), put(&k, "team-a", team_deposit())).await.status, StatusCode::OK);
    assert_eq!(send(app2, status(&k, "team-a")).await.json()["state"], "ready");
}

#[tokio::test]
async fn an_overlay_not_redeposited_for_the_ttl_is_purged() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", "d1").await.status, StatusCode::OK);
    let g = w.state.generator();
    assert_eq!(g.purge_at(SystemTime::now() + Duration::from_secs(13 * 86_400)).unwrap().ttl, 0);
    assert_eq!(w.state_of(&k, "team-a").await["state"], "ready");
    let r = g.purge_at(SystemTime::now() + Duration::from_secs(15 * 86_400)).unwrap();
    assert_eq!(r.ttl, 1);
    assert_eq!(w.state_of(&k, "team-a").await["state"], "absent");
    // Les dossiers vides sont retirés avec le dernier fichier.
    assert!(tree(&w.store_dir()).is_empty());
    let leftover: Vec<_> = std::fs::read_dir(
        w.store_dir().join(std::fs::read_dir(w.store_dir()).unwrap().next().unwrap().unwrap().file_name()),
    )
    .unwrap()
    .collect();
    assert!(leftover.is_empty(), "plus de dossier d'espace vide");
}

// --- Chiffrement -----------------------------------------------------------------------------------------------------

#[tokio::test]
async fn files_are_encrypted_bound_to_their_name_and_tampering_reads_as_absent() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-secret-a", "d1").await.status, StatusCode::OK);
    assert_eq!(w.deposit(&k, "team-secret-b", "d2").await.status, StatusCode::OK);
    let files = w.overlays();
    assert_eq!(files.len(), 2);
    for f in files.iter().chain(&w.profiles()) {
        let bytes = std::fs::read(w.store_dir().join(f)).unwrap();
        // Une surcouche et un profil ont chacun leur magie (3b).
        assert_eq!(&bytes[..8], if f.ends_with(".ordo.enc") { b"ORDOENC1" } else { b"ORDPENC1" });
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(!has(b"ORDOVRL1"), "la magie de la surcouche en clair");
        assert!(!has(b"team-secret"), "la référence d'équipe en clair");
    }
    // Un octet modifié : absente (et le fichier est effacé).
    let victim = w.store_dir().join(&files[0]);
    let mut bytes = std::fs::read(&victim).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x40;
    std::fs::write(&victim, &bytes).unwrap();
    let states: Vec<_> = {
        let a = w.state_of(&k, "team-secret-a").await["state"].as_str().unwrap().to_owned();
        let b = w.state_of(&k, "team-secret-b").await["state"].as_str().unwrap().to_owned();
        vec![a, b]
    };
    assert_eq!(states.iter().filter(|s| *s == "absent").count(), 1, "{states:?}");
    assert_eq!(states.iter().filter(|s| *s == "ready").count(), 1, "{states:?}");
    assert_eq!(w.overlays().len(), 1, "le fichier corrompu est effacé");
    // Fichiers échangés : chacun ne se déchiffre que sous son propre nom.
    assert_eq!(w.deposit(&k, "team-secret-a", "d1").await.status, StatusCode::OK);
    assert_eq!(w.deposit(&k, "team-secret-b", "d2").await.status, StatusCode::OK);
    let files = w.overlays();
    assert_eq!(files.len(), 2);
    let (pa, pb) = (w.store_dir().join(&files[0]), w.store_dir().join(&files[1]));
    let (ba, bb) = (std::fs::read(&pa).unwrap(), std::fs::read(&pb).unwrap());
    std::fs::write(&pa, bb).unwrap();
    std::fs::write(&pb, ba).unwrap();
    assert_eq!(w.state_of(&k, "team-secret-a").await["state"], "absent");
    assert_eq!(w.state_of(&k, "team-secret-b").await["state"], "absent");
}

// --- Bornes -----------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn deposit_bounds_document_body_and_gzip_inflation() {
    let w = World::with(|c| {
        c.generator.deposit.max_document_bytes = 2_000;
        c.generator.deposit.max_body_bytes = 50_000;
    });
    let k = key(1);
    let doc = document(21, &[6, 8, 10, 12, 14], &[6, 8, 10, 12, 14]);
    let big = track_line("ROAD", false, &doc);
    assert!(big.len() > 2_000);
    // Un document au-dessus de la borne d'une ligne.
    let r = w.send(put(&k, "t", ndjson(std::slice::from_ref(&big), Some(1)))).await;
    let body = assert_problem(&r, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(body["details"]["limit"], "document_bytes");
    // Le même cas, sans saut de ligne du tout (ligne en cours qui ne finit pas).
    let r = w.send(put(&k, "t", big.clone())).await;
    assert_eq!(
        assert_problem(&r, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large")["details"]["limit"],
        "document_bytes"
    );
    // Un corps gzip minuscule qui gonfle au-delà de la borne du corps décompressé.
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let small = track_line("ROAD", false, &json!({"edges": []}));
    for _ in 0..4_000 {
        gz.write_all(small.as_bytes()).unwrap();
        gz.write_all(b"\n").unwrap();
    }
    let packed = gz.finish().unwrap();
    assert!(packed.len() < 50_000, "compressé sous la borne : {}", packed.len());
    let mut req = put(&k, "t", packed);
    req.headers_mut().insert("content-encoding", "gzip".parse().unwrap());
    let r = w.send(req).await;
    let body = assert_problem(&r, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(body["details"]["limit"], "body_bytes");
    // Un corps gzip valide dans les bornes passe.
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(ndjson(&[track_line("ROAD", false, &json!({"edges": []}))], Some(1)).as_bytes()).unwrap();
    let mut req = put(&k, "t", gz.finish().unwrap());
    req.headers_mut().insert("content-encoding", "gzip".parse().unwrap());
    let r = w.send(req).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["tracks"], 1);
    // Mauvais type de contenu.
    let mut req = put(&k, "t", "x");
    req.headers_mut().insert("content-type", "application/json".parse().unwrap());
    assert_problem(&w.send(req).await, StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
}

#[tokio::test]
async fn deposit_reports_what_was_read_and_counts_excluded_and_unreadable_tracks() {
    let w = World::new();
    let k = key(1);
    let doc = document(21, &[6, 8, 10, 12, 14], &[6, 8, 10, 12, 14]);
    let lines = vec![
        track_line("ROAD", false, &doc),
        track_line("ROAD", true, &doc),
        json!({"type": "GRAVEL", "excluded": false, "network_edges": null}).to_string(),
        track_line("autre", false, &json!({"edges": [{"g": "pas une polyligne", "trv": 10.0}]})),
    ];
    let r = w.send(put_digest(&k, "team-a", "abc", ndjson(&lines, Some(4)))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let j = r.json();
    assert_eq!(
        (j["tracks"].as_u64(), j["tracks_excluded"].as_u64(), j["tracks_unreadable"].as_u64()),
        (Some(4), Some(1), Some(1))
    );
    assert!(j["edges"].as_u64().unwrap() > 50, "{j}");
    assert!(j["km"]["matched"].as_f64().unwrap() > 10.0, "{j}");
    assert_eq!(j["km"]["unmatched"]["invalid"].as_f64().unwrap(), 0.01);
    assert_eq!(j["source_digest"], "abc");
    let s = w.state_of(&k, "team-a").await;
    assert_eq!((s["state"].as_str(), s["edges"].as_u64()), (Some("ready"), j["edges"].as_u64()));
    assert!(s["expires_at"].as_str().unwrap() > s["deposited_at"].as_str().unwrap());
}

#[tokio::test]
async fn storage_quotas_answer_507() {
    let w = World::with(|c| c.generator.storage.max_overlays_per_namespace = 1);
    let k = key(1);
    assert_eq!(w.deposit(&k, "a", "d").await.status, StatusCode::OK);
    // Remplacer ne compte pas deux fois ; une seconde équipe dépasse.
    assert_eq!(w.deposit(&k, "a", "d").await.status, StatusCode::OK);
    assert_problem(&w.deposit(&k, "b", "d").await, StatusCode::INSUFFICIENT_STORAGE, "insufficient_storage");
    let w = World::with(|c| c.generator.storage.max_namespaces = 1);
    assert_eq!(w.deposit(&key(1), "a", "d").await.status, StatusCode::OK);
    assert_problem(
        &w.deposit(&key(2), "a", "d").await,
        StatusCode::INSUFFICIENT_STORAGE,
        "insufficient_storage",
    );
    assert_eq!(w.deposit(&key(1), "b", "d").await.status, StatusCode::OK);
}

// --- Génération -----------------------------------------------------------------------------------------------------

#[tokio::test]
async fn cursor_maps_to_modes_and_lambda_and_is_validated() {
    let w = World::new();
    let (k, team) = (key(1), "team-a");
    assert_eq!(w.deposit(&k, team, "d1").await.status, StatusCode::OK);
    let run = |c: f64| {
        let (w, k) = (&w, k.clone());
        async move { w.send(generate(Some(&k), Some(team), &generate_body(c))).await }
    };
    let strict = run(0.0).await;
    assert_eq!(strict.status, StatusCode::OK, "{}", strict.text);
    let j = strict.json();
    assert_eq!((j["mode"].as_str(), j["lambda"].is_null()), (Some("strict"), true));
    for route in j["routes"].as_array().unwrap() {
        assert!(
            route["team_share"].as_f64().unwrap() > 0.99,
            "strict = routes de l'équipe seulement : {route}"
        );
    }
    let mid = run(0.5).await.json();
    assert_eq!((mid["mode"].as_str(), mid["lambda"].as_f64()), (Some("team_first"), Some(2.0)));
    let low = run(0.1).await.json();
    assert_eq!(low["lambda"].as_f64(), Some(2.8));
    let disc = run(1.0).await.json();
    assert_eq!((disc["mode"].as_str(), disc["lambda"].as_f64()), (Some("discovery"), Some(1.0)));
    assert_eq!(disc["graph_id"], mid["graph_id"]);
    assert!(disc["overlay"]["deposited_at"].is_string(), "à 1 avec l'équipe : parts d'équipe mesurées");
    // Découverte sans clé ni équipe : permis, sans surcouche.
    let anon = w.send(generate(None, None, &generate_body(1.0))).await;
    assert_eq!(anon.status, StatusCode::OK, "{}", anon.text);
    assert!(anon.json()["overlay"].is_null());
    // Sous 1, les en-têtes sont obligatoires.
    assert_problem(
        &w.send(generate(None, None, &generate_body(0.5))).await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
    );
    // Hors de [0, 1] : 400 avec le pointeur.
    for bad in [1.01, -0.01] {
        let r = run(bad).await;
        let body = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(body["details"]["errors"][0]["pointer"], "/cursor");
    }
    // Distance hors bornes, latitude absurde, clé inconnue.
    let mut b = generate_body(1.0);
    b["distance_m"] = json!(500.0);
    let body =
        assert_problem(&w.send(generate(None, None, &b)).await, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(body["details"]["errors"][0]["pointer"], "/distance_m");
    let mut b = generate_body(1.0);
    b["start"]["lat"] = json!(91.0);
    assert_problem(&w.send(generate(None, None, &b)).await, StatusCode::BAD_REQUEST, "invalid_input");
    let mut b = generate_body(1.0);
    b["familiarity"] = json!(0.5);
    let body =
        assert_problem(&w.send(generate(None, None, &b)).await, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(body["details"]["unknown_keys"][0], "/familiarity");
    // Corps au-dessus de 64 Kio.
    let r = w.send(generate(None, None, &json!({"pad": "x".repeat(70_000)}))).await;
    assert_problem(&r, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
}

#[tokio::test]
async fn the_independent_checker_runs_on_every_route_and_drops_failures() {
    let w = World::new();
    let (k, team) = (key(1), "team-a");
    assert_eq!(w.deposit(&k, team, "d1").await.status, StatusCode::OK);
    let body = generate_body(0.5);
    let ok = w.send(generate(Some(&k), Some(team), &body)).await.json();
    assert!(!ok["routes"].as_array().unwrap().is_empty());
    assert_eq!(ok["dropped"], 0);
    let r0 = &ok["routes"][0];
    assert!(r0["polyline"].as_str().unwrap().len() > 20);
    assert!(r0["ascent_m"].is_null() && r0["descent_m"].is_null());
    assert!((r0["distance_m"].as_f64().unwrap() - 3600.0).abs() <= 0.1 * 3600.0 + 1.0);
    // Crochet de test : chaque parcours est altéré avant le contrôleur → tous retirés, comptés, 422.
    let hooks = w.state.generator().hooks().unwrap();
    hooks.corrupt_routes.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = w.send(generate(Some(&k), Some(team), &body)).await;
    let problem = assert_problem(&r, StatusCode::UNPROCESSABLE_ENTITY, "generation_failed");
    assert_eq!(problem["details"]["reason"], "checker_rejected");
    hooks.corrupt_routes.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(w.send(generate(Some(&k), Some(team), &body)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn coverage_is_explicit_in_errors_and_in_capabilities() {
    let w = World::new();
    // Hors du polygone.
    let mut b = generate_body(1.0);
    b["start"] = json!({"lat": 48.5, "lon": 2.3});
    let r = w.send(generate(None, None, &b)).await;
    let body = assert_problem(&r, StatusCode::UNPROCESSABLE_ENTITY, "out_of_coverage");
    assert_eq!(body["details"]["coverage"], "graph");
    assert!(!r.text.contains("48.5"), "aucune coordonnée dans l'erreur");
    // Arrivée hors du polygone.
    let mut b = generate_body(1.0);
    b["end"] = json!({"lat": 48.5, "lon": 2.3});
    assert_problem(
        &w.send(generate(None, None, &b)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "out_of_coverage",
    );
    // Dans le polygone mais disque D/2 qui en sort : `partial`.
    let p = node(1, 1);
    let mut b = generate_body(1.0);
    b["start"] = json!({"lat": p[0], "lon": p[1]});
    b["distance_m"] = json!(3600.0);
    let r = w.send(generate(None, None, &b)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["coverage"], "partial");
    assert_eq!(w.send(generate(None, None, &generate_body(1.0))).await.json()["coverage"], "inside");
    // Capacités : ce qui est couvert, avec ses limites.
    let c = w.send(get("/v1/capabilities")).await.json();
    let g = &c["generator"];
    assert_eq!(c["api_version"], "1.13");
    assert_eq!(g["enabled"], true);
    assert!(g["graph"]["data_date"].as_str().unwrap().ends_with("Z"));
    assert!(g["graph"]["id"].as_str().unwrap().len() == 16);
    assert_eq!(g["coverage"]["polygon"]["type"], "MultiPolygon");
    assert_eq!(g["coverage"]["bbox"].as_array().unwrap().len(), 4);
    assert_eq!(g["ride_types"], json!(["road", "gravel", "mtb"]));
    assert_eq!(g["cursor"]["lambda_max"], 3.0);
    assert_eq!(g["cursor"]["lambda_min"], 1.0);
    assert_eq!(g["cursor"]["strict_at"], 0.0);
    assert_eq!(g["cursor"]["discovery_at"], 1.0);
    assert_eq!(g["limits"]["distance_m_min"], 1000.0);
    // Budget d'une génération publié : 30 s par défaut depuis 1.14.0 (8 s avant).
    assert_eq!(g["limits"]["budget_ms"], 30_000);
    assert_eq!(g["limits"]["deposit"]["max_tracks"], 20_000);
    // Le quota qui décide d'un 507 est celui de l'espace : il est publié, comme l'attente de la place de dépôt.
    assert_eq!(g["limits"]["storage"]["max_namespace_mb"], 64);
    assert_eq!(g["limits"]["deposit"]["queue_wait_s"], 120);
    assert_eq!(g["overlay_ttl_days"], 14);
    assert_eq!(g["ascent"], false);
    // 4c : les figures offertes et celle d'une requête qui n'en nomme pas (organique depuis 1.12).
    assert_eq!(g["figures"], json!({"default": "organic", "values": ["classic", "organic"]}));
    assert!(g["matcher_hash"].as_str().unwrap().len() == 16);
}

#[tokio::test]
async fn without_a_graph_the_service_starts_serves_elevation_and_the_generator_answers_503() {
    for (name, tweak) in [
        ("sans chemin", Box::new(|_: &mut Config| {}) as Box<dyn Fn(&mut Config)>),
        ("fichier absent", Box::new(|c: &mut Config| c.generator.graph = Some("/nonexistent/g.ordg".into()))),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let files = write_files(dir.path(), "g", 21);
        let mut config = if name == "sans chemin" {
            test_config()
        } else {
            config_with(test_config(), dir.path(), &files)
        };
        tweak(&mut config);
        let app = router(AppState::new(&config).expect("le service démarre"));
        let caps = send(app.clone(), get("/v1/capabilities")).await.json();
        assert_eq!(caps["generator"]["enabled"], false, "{name}");
        assert!(caps["generator"]["reason"].is_string(), "{name}");
        assert!(caps["generator"].get("coverage").is_none());
        assert_eq!(send(app.clone(), get("/health/ready")).await.status, StatusCode::OK, "{name}");
        // L'altitude est servie (ici : 503 de tuiles hors ligne, comme sans générateur, et non 404 ni 500).
        let valid = ord_testkit::routes::request_body(
            &ord_testkit::routes::meridian(ord_testkit::routes::START, 1_000.0, 3),
            None,
        );
        let r = send(app.clone(), post_json(&valid)).await;
        assert_passed_validation(&r);
        let r = send(app.clone(), generate(None, None, &generate_body(1.0))).await;
        let body = assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
        assert_eq!(body["details"]["source"], "graph", "{name}");
        assert_eq!(r.headers.get("retry-after").unwrap(), "3600");
        let r = send(app.clone(), status(&key(1), "t")).await;
        assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
        assert_problem(
            &send(app, put(&key(1), "t", team_deposit())).await,
            StatusCode::SERVICE_UNAVAILABLE,
            "data_unavailable",
        );
    }
}

#[tokio::test]
async fn a_view_of_another_graph_or_a_polygon_far_from_the_graph_disables_the_generator() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_files(dir.path(), "a", 21);
    let b = write_files(dir.path(), "b", 20);
    let mut config = config_with(test_config(), dir.path(), &a);
    config.generator.view_road = Some(b.road.clone());
    let caps = send(router(AppState::new(&config).unwrap()), get("/v1/capabilities")).await.json();
    assert_eq!(
        (caps["generator"]["enabled"].as_bool(), caps["generator"]["reason"].as_str()),
        (Some(false), Some("view_mismatch"))
    );
    // Polygone à 100 km du graphe.
    let far = dir.path().join("far.poly");
    std::fs::write(&far, "t\n1\n 0.0 0.0\n 0.1 0.0\n 0.1 0.1\n 0.0 0.1\n 0.0 0.0\nEND\nEND\n").unwrap();
    let mut config = config_with(test_config(), dir.path(), &a);
    config.generator.coverage_poly = Some(far);
    let caps = send(router(AppState::new(&config).unwrap()), get("/v1/capabilities")).await.json();
    assert_eq!(caps["generator"]["reason"], "polygon_mismatch");
}

/// Le fichier d'index de service (note `ord-loops-planete.md` §5.3) : à côté du graphe, il sert les mêmes parcours que
/// les index calculés au démarrage ; celui d'un autre graphe éteint le générateur.
#[tokio::test]
async fn the_service_index_file_serves_the_same_routes_and_one_of_another_graph_disables_the_generator() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_files(dir.path(), "a", 21);
    let body = json!({"start": {"lat": node(10, 10)[0], "lon": node(10, 10)[1]}, "distance_m": 3600.0,
        "ride_type": "road", "cursor": 1.0, "seed": 1});
    let routes = |config: &Config| {
        let app = router(AppState::new(config).unwrap());
        let body = body.clone();
        async move {
            let caps = send(app.clone(), get("/v1/capabilities")).await.json();
            assert_eq!(caps["generator"]["enabled"], true, "{caps}");
            let r = send(app, generate(None, None, &body)).await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text);
            r.json()["routes"].clone()
        }
    };
    let config = config_with(test_config(), dir.path(), &a);
    let computed = routes(&config).await;
    assert!(!computed.as_array().unwrap().is_empty());
    write_service_index(&a, &a.graph.with_extension("ordi"));
    let from_file = routes(&config).await;
    assert_eq!(from_file, computed);
    // Un chemin explicite marche aussi.
    let moved = dir.path().join("ailleurs.ordi");
    std::fs::rename(a.graph.with_extension("ordi"), &moved).unwrap();
    let mut explicit = config.clone();
    explicit.generator.index = Some(moved.clone());
    assert_eq!(routes(&explicit).await, computed);

    // Le fichier d'un autre graphe, à la place de celui-ci : éteint, sans recalculer.
    let b = write_files(dir.path(), "b", 20);
    write_service_index(&b, &a.graph.with_extension("ordi"));
    let caps = send(router(AppState::new(&config).unwrap()), get("/v1/capabilities")).await.json();
    assert_eq!(
        (caps["generator"]["enabled"].as_bool(), caps["generator"]["reason"].as_str()),
        (Some(false), Some("graph_unreadable"))
    );
    // Un fichier tronqué : de même.
    std::fs::remove_file(a.graph.with_extension("ordi")).unwrap();
    let bytes = std::fs::read(&moved).unwrap();
    std::fs::write(a.graph.with_extension("ordi"), &bytes[..bytes.len() / 2]).unwrap();
    let caps = send(router(AppState::new(&config).unwrap()), get("/v1/capabilities")).await.json();
    assert_eq!(caps["generator"]["reason"], "graph_unreadable");
}

#[tokio::test]
async fn a_stored_overlay_of_the_graph_is_not_the_one_of_the_contract_version_alone() {
    // Les routes sont dans le contrat avec leurs en-têtes, et le chemin ne porte aucun identifiant.
    let doc = serde_json::to_value(ord_server::openapi()).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    for p in ["/v1/generator/overlay", "/v1/generator/overlays", "/v1/generator/routes"] {
        assert!(paths.contains_key(p), "{p}");
        assert!(!p.contains('{'), "aucun paramètre de chemin : {p}");
    }
    let put = &doc["paths"]["/v1/generator/overlay"]["put"];
    let names: Vec<&str> =
        put["parameters"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Ord-Namespace-Key", "Ord-Team", "Ord-Source-Digest"]);
    assert_eq!(doc["info"]["version"], "1.13");
}

// --- Revue : file, budget, équité, quotas -----------------------------------------------------------------------------

/// Un corps qui envoie une ligne de trace toutes les `every`, sans fin : jamais de silence, jamais de fin.
struct Trickle {
    line: Vec<u8>,
    every: Duration,
    sleep: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl Trickle {
    fn body(line: String, every: Duration) -> axum::body::Body {
        axum::body::Body::new(Trickle {
            line: format!("{line}\n").into_bytes(),
            every,
            sleep: Box::pin(tokio::time::sleep(every)),
        })
    }
}

impl http_body::Body for Trickle {
    type Data = axum::body::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<axum::body::Bytes>, Self::Error>>> {
        if std::future::Future::poll(self.sleep.as_mut(), cx).is_pending() {
            return std::task::Poll::Pending;
        }
        let next = tokio::time::Instant::now() + self.every;
        self.sleep.as_mut().reset(next);
        let data = axum::body::Bytes::from(self.line.clone());
        std::task::Poll::Ready(Some(Ok(http_body::Frame::data(data))))
    }
}

#[tokio::test]
async fn a_dropped_queued_generation_gives_back_its_place_in_the_queue() {
    let w = World::with(|c| {
        c.generator.max_concurrent = 1;
        c.generator.max_queued = 1;
    });
    let g = w.state.generator();
    let held = g.test_compute_slot(Duration::from_secs(1)).await.expect("prêt").expect("place libre");
    // Plus de lâchers que la file n'a de places : si chacun gardait la sienne, la file serait morte.
    for _ in 0..4 {
        let waiter = {
            let g = g.clone();
            tokio::spawn(async move { g.test_compute_slot(Duration::from_secs(30)).await.map(|r| r.is_ok()) })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        waiter.abort();
        let _ = waiter.await;
    }
    // La file accepte encore une attente (elle finit en 504 faute de place, pas en 503 immédiat).
    let r = g.test_compute_slot(Duration::from_millis(200)).await.unwrap();
    assert_eq!(r.expect_err("pas de place").code, ord_server::problem::ProblemCode::Timeout);
    drop(held);
    assert!(g.test_compute_slot(Duration::from_millis(200)).await.unwrap().is_ok());
}

#[tokio::test]
async fn an_environment_cannot_keep_the_deposit_slot_past_the_budget_by_trickling_bytes() {
    let w = World::with(|c| {
        c.generator.deposit.budget_s = 1;
        c.generator.deposit.idle_timeout_s = 30;
    });
    let doc = document(21, &[8], &[]);
    let line = track_line("ROAD", false, &doc);
    let t0 = std::time::Instant::now();
    // Des octets arrivent en continu : le silence ne se déclenche jamais, le budget si.
    let r = w.send(put(&key(1), "team-a", Trickle::body(line, Duration::from_millis(100)))).await;
    assert_problem(&r, StatusCode::GATEWAY_TIMEOUT, "timeout");
    assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());
    assert!(tree(&w.store_dir()).is_empty(), "rien d'écrit");
    // La place et le verrou sont rendus : un autre environnement dépose aussitôt.
    assert_eq!(w.send(put(&key(2), "team-a", team_deposit())).await.status, StatusCode::OK);
}

#[tokio::test]
async fn deposits_of_other_environments_queue_fairly_instead_of_being_refused() {
    let w = World::with(|c| c.generator.deposit.queue_wait_s = 5);
    let (k1, k2) = (key(1), key(2));
    let doc = document(21, &[8], &[]);
    let first_line = ndjson(&[track_line("ROAD", false, &doc)], None).into_bytes();
    let app = w.app.clone();
    let req = put(&k1, "team-a", Frames::new(vec![first_line]).then_stall().body());
    let held = tokio::spawn(async move { send(app, req).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    // L'autre environnement attend sa place ; quand le premier part, il passe.
    let app = w.app.clone();
    let req2 = put(&k2, "team-a", team_deposit());
    let queued = tokio::spawn(async move { send(app, req2).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!queued.is_finished(), "il attend, il n'est pas refusé");
    held.abort();
    let _ = held.await;
    let r = queued.await.unwrap();
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // Une attente qui dépasse son délai répond 503 avec `Retry-After`.
    let w = World::with(|c| c.generator.deposit.queue_wait_s = 1);
    let first_line = ndjson(&[track_line("ROAD", false, &doc)], None).into_bytes();
    let app = w.app.clone();
    let req = put(&k1, "team-a", Frames::new(vec![first_line]).then_stall().body());
    let held = tokio::spawn(async move { send(app, req).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let r = w.send(put(&k2, "team-a", team_deposit())).await;
    assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE, "overloaded");
    assert!(r.headers.get("retry-after").is_some());
    held.abort();
    let _ = held.await;
}

#[tokio::test]
async fn a_queued_deposit_asks_for_its_body_at_once_so_that_100_continue_is_not_held_back() {
    // Le serveur HTTP ne répond « 100 Continue » qu'à la première lecture du corps. Faite après la file, elle arrivait
    // trop tard pour le client du JDK, qui prenait ce « 100 » pour la réponse et n'entendait jamais la vraie.
    let w = World::with(|c| c.generator.deposit.queue_wait_s = 5);
    let (k1, k2) = (key(1), key(2));
    let doc = document(21, &[8], &[]);
    let first_line = ndjson(&[track_line("ROAD", false, &doc)], None).into_bytes();
    let app = w.app.clone();
    let req = put(&k1, "team-a", Frames::new(vec![first_line]).then_stall().body());
    let held = tokio::spawn(async move { send(app, req).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let polls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = w.app.clone();
    let whole = team_deposit().into_bytes();
    let (a, b) = whole.split_at(whole.len() / 2);
    let body = Frames::new(vec![a.to_vec(), b.to_vec()]).counting(std::sync::Arc::clone(&polls)).body();
    let req2 = put(&k2, "team-a", body);
    let queued = tokio::spawn(async move { send(app, req2).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!queued.is_finished(), "il attend sa place");
    // Une seule trame est lue d'avance : le reste du corps attend la place.
    assert_eq!(polls.load(std::sync::atomic::Ordering::Relaxed), 1, "le corps est demandé pendant l'attente");
    held.abort();
    let _ = held.await;
    // La trame lue d'avance n'est pas perdue : le dépôt entier est compté.
    let r = queued.await.unwrap();
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["tracks"], 2);
}

#[tokio::test]
async fn a_failed_directory_sync_after_the_rename_is_still_a_successful_deposit() {
    let w = World::new();
    let k = key(1);
    let hooks = w.state.generator().hooks().expect("prêt");
    hooks.fail_dir_sync.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = w.deposit(&k, "team-a", "d1").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(w.state_of(&k, "team-a").await["source_digest"], "d1", "la surcouche est bien en place");
}

#[tokio::test]
async fn a_very_long_line_in_small_frames_is_split_in_linear_time() {
    let w = World::with(|c| c.generator.deposit.max_document_bytes = 16 * 1024 * 1024);
    // Une ligne de 6 Mio livrée par trames de 4 Kio : illisible comme document (une chaîne), comptée comme telle.
    let pad = "x".repeat(6 * 1024 * 1024);
    let text = ndjson(&[format!(r#"{{"type":"ROAD","network_edges":"{pad}"}}"#)], Some(1)).into_bytes();
    let frames: Vec<Vec<u8>> = text.chunks(4096).map(<[u8]>::to_vec).collect();
    let t0 = std::time::Instant::now();
    let r = w.send(put(&key(1), "team-a", Frames::new(frames).body())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["tracks_unreadable"], 1);
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
}

#[tokio::test]
async fn a_stranger_cannot_take_a_namespace_slot_when_namespaces_are_listed() {
    use ord_server::generator::crypto::{decode_key, Credentials};
    let listed = Credentials::derive(&decode_key(&key(1)).unwrap(), "t").ns8().to_owned();
    let w = World::with(|c| c.generator.storage.allowed_namespaces = vec![listed.clone()]);
    // L'inconnu essaie, dans l'ordre, de remplir les places : toutes refusées, et la production passe.
    for n in 2..8 {
        assert_problem(
            &w.deposit(&key(n), "team-a", "d").await,
            StatusCode::INSUFFICIENT_STORAGE,
            "insufficient_storage",
        );
    }
    assert_eq!(w.deposit(&key(1), "team-a", "d").await.status, StatusCode::OK);
}

#[tokio::test]
async fn two_generations_of_the_same_cold_team_build_its_data_once() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", "d1").await.status, StatusCode::OK);
    // Redémarrage : le cache est vide ; deux demandes arrivent ensemble.
    let state = AppState::new(&w.config).unwrap();
    let app = router(state.clone());
    let body = generate_body(0.4);
    let (a, b) = tokio::join!(
        send(app.clone(), generate(Some(&k), Some("team-a"), &body)),
        send(app.clone(), generate(Some(&k), Some("team-a"), &body)),
    );
    assert_eq!((a.status, b.status), (StatusCode::OK, StatusCode::OK), "{} {}", a.text, b.text);
    let builds = state.generator().hooks().unwrap().team_builds.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(builds, 1);
}
