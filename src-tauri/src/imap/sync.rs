//! Reine Sync-Entscheidungslogik — **kein Netzwerk, kein SQLite**.
//!
//! Diese Funktionen entscheiden anhand von Server-Stand und Cache-Stand,
//! was zu tun ist. Sie sind vollständig unit-getestet; die dünne
//! Netzwerk-Schicht (`verbindung.rs`) führt die Entscheidungen nur aus.

/// Ergebnis des Abgleichs zwischen Server- und Cache-Stand eines Ordners.
#[derive(Debug, PartialEq, Eq)]
pub struct Abgleich {
    /// UIDs, die der Server hat, der Cache aber nicht — neueste zuerst.
    pub neue: Vec<u32>,
    /// UIDs, die der Cache hat, der Server aber nicht mehr.
    pub geloeschte: Vec<u32>,
    /// UIDs, deren Gelesen-Flag sich unterscheidet (Server gewinnt).
    pub flag_aenderungen: Vec<(u32, bool)>,
}

/// UIDVALIDITY-Regel: Ändert sich der Wert, sind alle gecachten UIDs
/// wertlos und der Ordner-Cache muss verworfen werden.
pub fn braucht_cache_reset(cache_uidvalidity: Option<u32>, server_uidvalidity: u32) -> bool {
    match cache_uidvalidity {
        None => false, // Erstsync: nichts zu verwerfen
        Some(gecacht) => gecacht != server_uidvalidity,
    }
}

/// Vergleicht Server-Stand (UID + Gelesen-Flag) mit dem Cache-Stand.
/// Beide Listen dürfen unsortiert sein.
pub fn vergleiche_ordner(server: &[(u32, bool)], cache: &[(u32, bool)]) -> Abgleich {
    use std::collections::HashMap;
    let server_map: HashMap<u32, bool> = server.iter().copied().collect();
    let cache_map: HashMap<u32, bool> = cache.iter().copied().collect();

    let mut neue: Vec<u32> = server_map
        .keys()
        .filter(|uid| !cache_map.contains_key(uid))
        .copied()
        .collect();
    neue.sort_unstable_by(|a, b| b.cmp(a)); // neueste (höchste UID) zuerst

    let mut geloeschte: Vec<u32> = cache_map
        .keys()
        .filter(|uid| !server_map.contains_key(uid))
        .copied()
        .collect();
    geloeschte.sort_unstable();

    let mut flag_aenderungen: Vec<(u32, bool)> = server_map
        .iter()
        .filter(|(uid, gelesen)| cache_map.get(uid).is_some_and(|c| c != *gelesen))
        .map(|(uid, gelesen)| (*uid, *gelesen))
        .collect();
    flag_aenderungen.sort_unstable();

    Abgleich {
        neue,
        geloeschte,
        flag_aenderungen,
    }
}

/// Teilt UIDs in Batches fester Größe auf (Reihenfolge bleibt erhalten —
/// bei absteigend sortierter Eingabe kommen die neuesten Mails zuerst).
pub fn batches(uids: &[u32], groesse: usize) -> Vec<Vec<u32>> {
    if groesse == 0 {
        return vec![uids.to_vec()];
    }
    uids.chunks(groesse).map(<[u32]>::to_vec).collect()
}

/// Baut aus UIDs die IMAP-Sequenzmenge, z. B. `3,5,7:9` — zusammenhängende
/// Bereiche werden zusammengefasst, damit die Anfrage kurz bleibt.
pub fn uid_sequenz(uids: &[u32]) -> String {
    let mut sortiert = uids.to_vec();
    sortiert.sort_unstable();
    sortiert.dedup();

    let mut teile: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sortiert.len() {
        let start = sortiert[i];
        let mut ende = start;
        while i + 1 < sortiert.len() && sortiert[i + 1] == ende + 1 {
            i += 1;
            ende = sortiert[i];
        }
        if start == ende {
            teile.push(start.to_string());
        } else {
            teile.push(format!("{start}:{ende}"));
        }
        i += 1;
    }
    teile.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erstsync_braucht_keinen_reset() {
        assert!(!braucht_cache_reset(None, 123));
    }

    #[test]
    fn gleiche_uidvalidity_braucht_keinen_reset() {
        assert!(!braucht_cache_reset(Some(123), 123));
    }

    #[test]
    fn geaenderte_uidvalidity_braucht_reset() {
        assert!(braucht_cache_reset(Some(123), 124));
    }

    #[test]
    fn abgleich_erkennt_neue_geloeschte_und_flags() {
        // Server: 1 (gelesen), 3 (ungelesen), 4 (gelesen)
        // Cache:  1 (ungelesen → Flag-Änderung), 2 (→ gelöscht), 3 (gleich)
        let server = [(1, true), (3, false), (4, true)];
        let cache = [(1, false), (2, false), (3, false)];
        let abgleich = vergleiche_ordner(&server, &cache);
        assert_eq!(abgleich.neue, vec![4]);
        assert_eq!(abgleich.geloeschte, vec![2]);
        assert_eq!(abgleich.flag_aenderungen, vec![(1, true)]);
    }

    #[test]
    fn abgleich_leerer_cache_liefert_alles_neueste_zuerst() {
        let server = [(5, false), (2, true), (9, false)];
        let abgleich = vergleiche_ordner(&server, &[]);
        assert_eq!(abgleich.neue, vec![9, 5, 2]);
        assert!(abgleich.geloeschte.is_empty());
        assert!(abgleich.flag_aenderungen.is_empty());
    }

    #[test]
    fn abgleich_leerer_server_loescht_alles() {
        let cache = [(1, false), (2, true)];
        let abgleich = vergleiche_ordner(&[], &cache);
        assert!(abgleich.neue.is_empty());
        assert_eq!(abgleich.geloeschte, vec![1, 2]);
    }

    #[test]
    fn batches_teilen_korrekt() {
        let uids: Vec<u32> = (1..=5).collect();
        assert_eq!(batches(&uids, 2), vec![vec![1, 2], vec![3, 4], vec![5]]);
        assert_eq!(batches(&[], 2), Vec::<Vec<u32>>::new());
    }

    #[test]
    fn uid_sequenz_fasst_bereiche_zusammen() {
        assert_eq!(uid_sequenz(&[3, 5, 9, 8, 7]), "3,5,7:9");
        assert_eq!(uid_sequenz(&[1]), "1");
        assert_eq!(uid_sequenz(&[2, 1, 3]), "1:3");
        assert_eq!(uid_sequenz(&[4, 4, 4]), "4");
        assert_eq!(uid_sequenz(&[]), "");
    }
}
