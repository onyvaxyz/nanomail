//! Dünne IMAP-Netzwerk-Schicht auf `async-imap`.
//!
//! Immer TLS auf dem konfigurierten Port (Standard 993) — niemals
//! STARTTLS-Downgrade. Enthält bewusst keine Sync-Entscheidungen
//! (die liegen getestet in `sync.rs`) und loggt nie Mail-Inhalte
//! oder Zugangsdaten.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_imap::types::NameAttribute;
use futures::TryStreamExt;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

type Session = async_imap::Session<TlsStream<TcpStream>>;

/// Ein Ordner, wie ihn der Server per LIST meldet.
#[derive(Debug)]
pub struct OrdnerEintrag {
    pub name: String,
    pub anzeige_name: String,
    /// Sonderrolle aus SPECIAL-USE (RFC 6154), z. B. `gesendet`.
    pub rolle: Option<String>,
}

/// Zustand eines Ordners nach SELECT.
#[derive(Debug)]
pub struct OrdnerStatus {
    pub uidvalidity: u32,
    pub anzahl: u32,
}

/// Rohdaten eines Mail-Kopfs aus dem Fetch.
#[derive(Debug)]
pub struct KopfDaten {
    pub uid: u32,
    pub gelesen: bool,
    pub header: Vec<u8>,
}

pub struct ImapVerbindung {
    session: Session,
}

impl ImapVerbindung {
    /// Baut eine TLS-Verbindung auf und meldet sich an.
    pub async fn verbinden(host: &str, port: u16, benutzer: &str, passwort: &str) -> Result<Self> {
        let tls = tls_verbinden(host, port).await?;
        let client = async_imap::Client::new(tls);
        let session = client
            .login(benutzer, passwort)
            .await
            .map_err(|(fehler, _)| anyhow!("IMAP-Anmeldung abgelehnt: {fehler}"))?;
        tracing::info!(host, port, "IMAP-Anmeldung erfolgreich");
        Ok(Self { session })
    }

    /// Listet alle auswählbaren Ordner.
    pub async fn ordner_auflisten(&mut self) -> Result<Vec<OrdnerEintrag>> {
        let namen: Vec<_> = self
            .session
            .list(Some(""), Some("*"))
            .await
            .context("Ordnerliste anfragen (LIST)")?
            .try_collect()
            .await
            .context("Ordnerliste lesen")?;

        let mut ordner = Vec::new();
        for name in namen {
            if name
                .attributes()
                .iter()
                .any(|a| matches!(a, NameAttribute::NoSelect))
            {
                continue;
            }
            let voller_name = name.name().to_string();
            let letzter_teil = match name.delimiter() {
                Some(trenner) if !trenner.is_empty() => voller_name
                    .rsplit(trenner)
                    .next()
                    .unwrap_or(&voller_name)
                    .to_string(),
                _ => voller_name.clone(),
            };
            let anzeige_name = if voller_name.eq_ignore_ascii_case("INBOX") {
                "Posteingang".to_string()
            } else {
                letzter_teil
            };
            let rolle = name.attributes().iter().find_map(|attribut| {
                Some(match attribut {
                    NameAttribute::Sent => "gesendet",
                    NameAttribute::Drafts => "entwuerfe",
                    NameAttribute::Trash => "papierkorb",
                    NameAttribute::Junk => "spam",
                    NameAttribute::Archive => "archiv",
                    _ => return None,
                })
            });
            ordner.push(OrdnerEintrag {
                name: voller_name,
                anzeige_name,
                rolle: rolle.map(str::to_string),
            });
        }
        Ok(ordner)
    }

    /// Legt eine Nachricht (Rohbytes) als gelesen in einem Ordner ab —
    /// für die „Gesendet“-Ablage nach dem SMTP-Versand.
    pub async fn nachricht_ablegen(&mut self, ordner: &str, roh: &[u8]) -> Result<()> {
        self.session
            .append(ordner, Some("(\\Seen)"), None, roh)
            .await
            .with_context(|| format!("Nachricht in „{ordner}“ ablegen (APPEND)"))
    }

    /// Wählt einen Ordner aus; alle folgenden UID-Operationen beziehen
    /// sich auf ihn.
    pub async fn ordner_waehlen(&mut self, name: &str) -> Result<OrdnerStatus> {
        let postfach = self
            .session
            .select(name)
            .await
            .with_context(|| format!("Ordner „{name}“ auswählen"))?;
        let uidvalidity = postfach
            .uid_validity
            .ok_or_else(|| anyhow!("Server liefert keine UIDVALIDITY für „{name}“"))?;
        Ok(OrdnerStatus {
            uidvalidity,
            anzahl: postfach.exists,
        })
    }

    /// Liefert alle UIDs des gewählten Ordners samt Gelesen-Flag.
    pub async fn uid_stand(&mut self, anzahl: u32) -> Result<Vec<(u32, bool)>> {
        if anzahl == 0 {
            return Ok(Vec::new());
        }
        let fetches: Vec<_> = self
            .session
            .uid_fetch("1:*", "(UID FLAGS)")
            .await
            .context("UID-Stand anfragen")?
            .try_collect()
            .await
            .context("UID-Stand lesen")?;
        let mut stand = Vec::with_capacity(fetches.len());
        for fetch in &fetches {
            if let Some(uid) = fetch.uid {
                stand.push((uid, ist_gelesen(fetch)));
            }
        }
        Ok(stand)
    }

    /// Lädt die Kopfzeilen der angegebenen UIDs (setzt kein \Seen-Flag).
    pub async fn koepfe_laden(&mut self, uids: &[u32]) -> Result<Vec<KopfDaten>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequenz = super::sync::uid_sequenz(uids);
        let fetches: Vec<_> = self
            .session
            .uid_fetch(&sequenz, "(UID FLAGS RFC822.HEADER)")
            .await
            .context("Kopfzeilen anfragen")?
            .try_collect()
            .await
            .context("Kopfzeilen lesen")?;
        let mut koepfe = Vec::with_capacity(fetches.len());
        for fetch in &fetches {
            let Some(uid) = fetch.uid else { continue };
            koepfe.push(KopfDaten {
                uid,
                gelesen: ist_gelesen(fetch),
                header: fetch.header().unwrap_or_default().to_vec(),
            });
        }
        Ok(koepfe)
    }

    /// Lädt die komplette Nachricht (ohne das \Seen-Flag zu setzen —
    /// „gelesen“ wird bewusst separat gesetzt).
    pub async fn nachricht_laden(&mut self, uid: u32) -> Result<Vec<u8>> {
        let fetches: Vec<_> = self
            .session
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .await
            .context("Nachricht anfragen")?
            .try_collect()
            .await
            .context("Nachricht lesen")?;
        let fetch = fetches
            .iter()
            .find(|f| f.uid == Some(uid))
            .ok_or_else(|| anyhow!("Server lieferte die Nachricht nicht"))?;
        let body = fetch
            .body()
            .ok_or_else(|| anyhow!("Nachricht ohne Inhalt geliefert"))?;
        Ok(body.to_vec())
    }

    /// Setzt das \Seen-Flag auf dem Server (Quelle der Wahrheit).
    pub async fn als_gelesen_markieren(&mut self, uid: u32) -> Result<()> {
        let _antworten: Vec<_> = self
            .session
            .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)")
            .await
            .context("Gelesen-Flag setzen")?
            .try_collect()
            .await
            .context("Antwort auf Flag-Änderung lesen")?;
        Ok(())
    }

    /// Wartet per IMAP IDLE auf Neuigkeiten im gewählten Ordner.
    /// Liefert die Verbindung zurück plus `true`, wenn der Server etwas
    /// gemeldet hat (`false` = Zeit abgelaufen, einfach weiterlauschen).
    pub async fn warte_auf_neuigkeiten(self, dauer: std::time::Duration) -> Result<(Self, bool)> {
        use async_imap::extensions::idle::IdleResponse;
        let mut idle = self.session.idle();
        idle.init().await.context("IDLE starten")?;
        let (warten, _abbruch) = idle.wait_with_timeout(dauer);
        let antwort = warten.await.context("IDLE warten")?;
        let session = idle.done().await.context("IDLE beenden")?;
        Ok((
            Self { session },
            matches!(antwort, IdleResponse::NewData(_)),
        ))
    }

    pub async fn abmelden(mut self) {
        // Fehler beim Abmelden sind unkritisch — Verbindung fällt ohnehin zu.
        if let Err(fehler) = self.session.logout().await {
            tracing::debug!("IMAP-Abmeldung fehlgeschlagen: {fehler}");
        }
    }
}

fn ist_gelesen(fetch: &async_imap::types::Fetch) -> bool {
    fetch
        .flags()
        .any(|flag| matches!(flag, async_imap::types::Flag::Seen))
}

/// Baut die TLS-Verbindung mit Zertifikatsprüfung auf.
///
/// `NANOMAIL_EXTRA_CA` (Pfad zu einer PEM-Datei) fügt der Vertrauensliste
/// eine zusätzliche CA hinzu — für Tests mit lokalem Mailserver. Die
/// Zertifikatsprüfung selbst wird niemals abgeschaltet.
async fn tls_verbinden(host: &str, port: u16) -> Result<TlsStream<TcpStream>> {
    let mut wurzeln = rustls::RootCertStore::empty();
    wurzeln.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    if let Ok(pfad) = std::env::var("NANOMAIL_EXTRA_CA") {
        let mut anzahl = 0usize;
        for zertifikat in CertificateDer::pem_file_iter(&pfad)
            .with_context(|| format!("Zusatz-CA {pfad} lesen"))?
        {
            let zertifikat = zertifikat.context("Zusatz-CA parsen")?;
            wurzeln.add(zertifikat).context("Zusatz-CA übernehmen")?;
            anzahl += 1;
        }
        tracing::warn!(pfad, anzahl, "Zusätzliche Test-CA(s) geladen");
    }

    let konfig = rustls::ClientConfig::builder()
        .with_root_certificates(wurzeln)
        .with_no_client_auth();
    let verbinder = TlsConnector::from(Arc::new(konfig));

    let tcp = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("Server {host}:{port} nicht erreichbar"))?;
    let server_name = ServerName::try_from(host.to_string())
        .with_context(|| format!("Ungültiger Servername „{host}“"))?;
    let tls = verbinder
        .connect(server_name, tcp)
        .await
        .with_context(|| format!("TLS-Verbindung zu {host} fehlgeschlagen"))?;
    if port != 993 {
        tracing::info!(
            host,
            port,
            "IMAP über Nicht-Standard-Port verbunden (TLS aktiv)"
        );
    }
    Ok(tls)
}
