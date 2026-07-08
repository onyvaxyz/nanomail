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
    /// UIDs, deren Gelesen- oder Beantwortet-Flag sich unterscheidet
    /// (Server gewinnt) — jeweils mit dem neuen Stand beider Flags.
    pub flag_aenderungen: Vec<(u32, bool, bool)>,
}

/// UIDVALIDITY-Regel: Ändert sich der Wert, sind alle gecachten UIDs
/// wertlos und der Ordner-Cache muss verworfen werden.
pub fn braucht_cache_reset(cache_uidvalidity: Option<u32>, server_uidvalidity: u32) -> bool {
    match cache_uidvalidity {
        None => false, // Erstsync: nichts zu verwerfen
        Some(gecacht) => gecacht != server_uidvalidity,
    }
}

/// Vergleicht Server-Stand (UID + Gelesen-/Beantwortet-Flag) mit dem
/// Cache-Stand. Beide Listen dürfen unsortiert sein.
pub fn vergleiche_ordner(server: &[(u32, bool, bool)], cache: &[(u32, bool, bool)]) -> Abgleich {
    use std::collections::HashMap;
    let server_map: HashMap<u32, (bool, bool)> = server
        .iter()
        .map(|(uid, gelesen, beantwortet)| (*uid, (*gelesen, *beantwortet)))
        .collect();
    let cache_map: HashMap<u32, (bool, bool)> = cache
        .iter()
        .map(|(uid, gelesen, beantwortet)| (*uid, (*gelesen, *beantwortet)))
        .collect();

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

    let mut flag_aenderungen: Vec<(u32, bool, bool)> = server_map
        .iter()
        .filter(|(uid, flags)| cache_map.get(uid).is_some_and(|c| c != *flags))
        .map(|(uid, (gelesen, beantwortet))| (*uid, *gelesen, *beantwortet))
        .collect();
    flag_aenderungen.sort_unstable();

    Abgleich {
        neue,
        geloeschte,
        flag_aenderungen,
    }
}

/// Entscheidet anhand der BODYSTRUCTURE-Antwort des Servers, ob eine
/// Mail „echte“ Anhänge hat — dieselbe Regel wie beim vollständigen
/// Parsen (`anzeige::anhang_teile`): Anhang-Disposition zählt immer,
/// Nicht-Text-Teile ohne Content-ID ebenfalls; eingebettete `cid:`-Bilder
/// und die Text-/HTML-Körper zählen nicht.
pub fn struktur_hat_anhang(struktur: &async_imap::imap_proto::types::BodyStructure<'_>) -> bool {
    use async_imap::imap_proto::types::BodyStructure;

    let ist_anhang_disposition = |common: &async_imap::imap_proto::types::BodyContentCommon<'_>| {
        common
            .disposition
            .as_ref()
            .is_some_and(|d| d.ty.eq_ignore_ascii_case("attachment"))
    };

    match struktur {
        BodyStructure::Multipart { bodies, .. } => bodies.iter().any(struktur_hat_anhang),
        // Text-Teile sind normalerweise der Mail-Körper.
        BodyStructure::Text { common, .. } => ist_anhang_disposition(common),
        // Angehängte Nachricht (message/rfc822) ist immer ein Anhang.
        BodyStructure::Message { .. } => true,
        BodyStructure::Basic { common, other, .. } => {
            ist_anhang_disposition(common) || other.id.is_none()
        }
    }
}

/// Rät die Sonderrolle eines Ordners aus seinem (letzten) Namensteil —
/// Fallback für Server, die kein SPECIAL-USE (RFC 6154) melden. Damit
/// stimmen Icons und Papierkorb-Erkennung auch dort.
pub fn rolle_aus_name(kurzname: &str) -> Option<&'static str> {
    match kurzname.to_lowercase().as_str() {
        "gesendet" | "sent" | "sent items" | "sent messages" | "gesendete objekte" => {
            Some("gesendet")
        }
        "entwürfe" | "entwuerfe" | "drafts" => Some("entwuerfe"),
        "trash"
        | "papierkorb"
        | "deleted items"
        | "deleted messages"
        | "gelöscht"
        | "gelöschte elemente" => Some("papierkorb"),
        "spam" | "junk" | "junk-e-mail" => Some("spam"),
        "archiv" | "archive" | "archives" => Some("archiv"),
        _ => None,
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
        let server = [(1, true, false), (3, false, false), (4, true, false)];
        let cache = [(1, false, false), (2, false, false), (3, false, false)];
        let abgleich = vergleiche_ordner(&server, &cache);
        assert_eq!(abgleich.neue, vec![4]);
        assert_eq!(abgleich.geloeschte, vec![2]);
        assert_eq!(abgleich.flag_aenderungen, vec![(1, true, false)]);
    }

    #[test]
    fn abgleich_erkennt_beantwortet_aenderung() {
        // Nur das Beantwortet-Flag weicht ab (z. B. Antwort aus einem
        // anderen Programm) — der Server-Stand gewinnt.
        let server = [(1, true, true), (2, false, false)];
        let cache = [(1, true, false), (2, false, false)];
        let abgleich = vergleiche_ordner(&server, &cache);
        assert!(abgleich.neue.is_empty());
        assert!(abgleich.geloeschte.is_empty());
        assert_eq!(abgleich.flag_aenderungen, vec![(1, true, true)]);
    }

    #[test]
    fn abgleich_leerer_cache_liefert_alles_neueste_zuerst() {
        let server = [(5, false, false), (2, true, false), (9, false, false)];
        let abgleich = vergleiche_ordner(&server, &[]);
        assert_eq!(abgleich.neue, vec![9, 5, 2]);
        assert!(abgleich.geloeschte.is_empty());
        assert!(abgleich.flag_aenderungen.is_empty());
    }

    #[test]
    fn abgleich_leerer_server_loescht_alles() {
        let cache = [(1, false, false), (2, true, false)];
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
    fn rolle_aus_name_erkennt_gaengige_namen() {
        assert_eq!(rolle_aus_name("Trash"), Some("papierkorb"));
        assert_eq!(rolle_aus_name("Papierkorb"), Some("papierkorb"));
        assert_eq!(rolle_aus_name("Sent Items"), Some("gesendet"));
        assert_eq!(rolle_aus_name("Entwürfe"), Some("entwuerfe"));
        assert_eq!(rolle_aus_name("Junk"), Some("spam"));
        assert_eq!(rolle_aus_name("Archive"), Some("archiv"));
        assert_eq!(rolle_aus_name("INBOX"), None);
        assert_eq!(rolle_aus_name("Rechnungen"), None);
    }

    #[test]
    fn uid_sequenz_fasst_bereiche_zusammen() {
        assert_eq!(uid_sequenz(&[3, 5, 9, 8, 7]), "3,5,7:9");
        assert_eq!(uid_sequenz(&[1]), "1");
        assert_eq!(uid_sequenz(&[2, 1, 3]), "1:3");
        assert_eq!(uid_sequenz(&[4, 4, 4]), "4");
        assert_eq!(uid_sequenz(&[]), "");
    }

    mod struktur {
        use super::super::struktur_hat_anhang;
        use async_imap::imap_proto::types::{
            BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentDisposition,
            ContentEncoding, ContentType,
        };
        use std::borrow::Cow;

        fn common(
            ty: &'static str,
            subtype: &'static str,
            disposition: Option<&'static str>,
        ) -> BodyContentCommon<'static> {
            BodyContentCommon {
                ty: ContentType {
                    ty: Cow::Borrowed(ty),
                    subtype: Cow::Borrowed(subtype),
                    params: None,
                },
                disposition: disposition.map(|d| ContentDisposition {
                    ty: Cow::Borrowed(d),
                    params: None,
                }),
                language: None,
                location: None,
            }
        }

        fn einzelteil(content_id: Option<&'static str>) -> BodyContentSinglePart<'static> {
            BodyContentSinglePart {
                id: content_id.map(Cow::Borrowed),
                md5: None,
                description: None,
                transfer_encoding: ContentEncoding::Base64,
                octets: 1000,
            }
        }

        fn text_teil() -> BodyStructure<'static> {
            BodyStructure::Text {
                common: common("text", "plain", None),
                other: einzelteil(None),
                lines: 10,
                extension: None,
            }
        }

        #[test]
        fn reiner_text_hat_keinen_anhang() {
            assert!(!struktur_hat_anhang(&text_teil()));
        }

        #[test]
        fn pdf_ohne_content_id_ist_anhang() {
            let pdf = BodyStructure::Basic {
                common: common("application", "pdf", None),
                other: einzelteil(None),
                extension: None,
            };
            let mail = BodyStructure::Multipart {
                common: common("multipart", "mixed", None),
                bodies: vec![text_teil(), pdf],
                extension: None,
            };
            assert!(struktur_hat_anhang(&mail));
        }

        #[test]
        fn eingebettetes_cid_bild_ist_kein_anhang() {
            let inline_bild = BodyStructure::Basic {
                common: common("image", "png", Some("inline")),
                other: einzelteil(Some("<bild1@example.org>")),
                extension: None,
            };
            let mail = BodyStructure::Multipart {
                common: common("multipart", "related", None),
                bodies: vec![text_teil(), inline_bild],
                extension: None,
            };
            assert!(!struktur_hat_anhang(&mail));
        }

        #[test]
        fn anhang_disposition_zaehlt_immer() {
            // Auch ein Bild mit Content-ID ist Anhang, wenn es als
            // „attachment“ gekennzeichnet ist — und sogar ein Text-Teil.
            let bild = BodyStructure::Basic {
                common: common("image", "jpeg", Some("attachment")),
                other: einzelteil(Some("<foto@example.org>")),
                extension: None,
            };
            assert!(struktur_hat_anhang(&bild));

            let text_anhang = BodyStructure::Text {
                common: common("text", "plain", Some("attachment")),
                other: einzelteil(None),
                lines: 200,
                extension: None,
            };
            assert!(struktur_hat_anhang(&text_anhang));
        }
    }
}
