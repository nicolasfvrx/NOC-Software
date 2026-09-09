//! Reusable RDP destinations. Accounts and passwords belong to kiosks, never servers.
use crate::{
    web::{esc, page},
    AppState,
};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form, Json,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path as FilePath, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct RdpServer {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub ignore_certificate_errors: bool,
}
#[derive(Default, Deserialize)]
pub struct ServerForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    port: String,
    #[serde(default)]
    ignore_certificate_errors: Option<String>,
}
pub struct ServerStore {
    path: PathBuf,
    servers: Mutex<Vec<RdpServer>>,
}
impl ServerStore {
    pub fn load(path: &FilePath) -> Result<Self, String> {
        let servers: Vec<RdpServer> = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Catalogue RDP invalide : {e}"))?
        } else {
            Vec::new()
        };
        let mut ids = std::collections::HashSet::new();
        for s in &servers {
            validate(s)?;
            if !s.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                || s.id.is_empty()
                || !ids.insert(s.id.clone())
            {
                return Err("Identifiant de serveur RDP invalide ou dupliqué".into());
            }
        }
        Ok(Self {
            path: path.into(),
            servers: Mutex::new(servers),
        })
    }
    pub fn all(&self) -> Vec<RdpServer> {
        self.servers.lock().unwrap().clone()
    }
    pub fn get(&self, id: &str) -> Option<RdpServer> {
        self.servers
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }
    pub fn save(&self, id: Option<&str>, form: &ServerForm) -> Result<RdpServer, String> {
        let mut servers = self.servers.lock().unwrap();
        let mut next = servers.clone();
        let server_id = if let Some(id) = id {
            if !next.iter().any(|s| s.id == id) {
                return Err("Serveur introuvable".into());
            }
            id.to_string()
        } else {
            let mut n = 1;
            while next.iter().any(|s| s.id == format!("rdp-{n}")) {
                n += 1;
            }
            format!("rdp-{n}")
        };
        let server = RdpServer {
            id: server_id,
            name: form.name.trim().into(),
            address: form.address.trim().into(),
            port: if form.port.trim().is_empty() {
                3389
            } else {
                form.port.trim().parse().unwrap_or(0)
            },
            ignore_certificate_errors: form.ignore_certificate_errors.is_some(),
        };
        validate(&server)?;
        if next
            .iter()
            .any(|s| s.id != server.id && s.name.eq_ignore_ascii_case(&server.name))
        {
            return Err("Un serveur porte déjà ce nom".into());
        }
        if let Some(existing) = next.iter_mut().find(|s| s.id == server.id) {
            *existing = server.clone();
        } else {
            next.push(server.clone());
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Écriture du catalogue impossible : {e}"))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("Enregistrement du catalogue impossible : {e}"))?;
        *servers = next;
        Ok(server)
    }
}
fn validate(s: &RdpServer) -> Result<(), String> {
    if s.name.is_empty() {
        return Err("Le nom du serveur est obligatoire".into());
    }
    if s.address.is_empty()
        || s.address.chars().any(char::is_whitespace)
        || s.address.contains("://")
        || s.address.contains(['@', '/', '\\', '?', '#'])
    {
        return Err("Indiquez une adresse IP ou un nom DNS, sans URL ni identifiants".into());
    }
    if s.port == 0 {
        return Err("Le port doit être compris entre 1 et 65535".into());
    }
    Ok(())
}
pub async fn list(State(state): State<Arc<AppState>>) -> Html<String> {
    let kiosks = state.storage.all();
    let rows=state.rdp_servers.all().iter().map(|s|format!(r#"<tr><td>{}</td><td><code>{}:{}</code></td><td>{}</td><td><a class="btn" href="/rdp-servers/{}/edit">Modifier</a></td></tr>"#,esc(&s.name),esc(&s.address),s.port,kiosks.iter().filter(|k|k.rdp.server_id.as_deref()==Some(&s.id)).count(),s.id)).collect::<String>();
    Html(page(
        "Serveurs RDP",
        &format!(
            r#"<div class="page-heading"><div><h1>Serveurs RDP</h1><p class="muted">Destinations Linux réutilisables. Les comptes et mots de passe se renseignent dans chaque kiosque.</p></div><a class="btn primary" href="/rdp-servers/new">+ Ajouter un serveur</a></div><div class="table-wrap"><table><thead><tr><th>Nom</th><th>Adresse / port</th><th>Kiosques associés</th><th></th></tr></thead><tbody>{}</tbody></table></div>"#,
            if rows.is_empty() {
                "<tr><td colspan=\"4\">Aucun serveur enregistré. Ajoutez votre première destination RDP.</td></tr>".into()
            } else {
                rows
            }
        ),
    ))
}
fn server_form(id: Option<&str>, form: &ServerForm, error: Option<&str>) -> Html<String> {
    let action = id
        .map(|id| format!("/rdp-servers/{id}/edit"))
        .unwrap_or("/rdp-servers/new".into());
    let title = if id.is_some() {
        "Modifier le serveur RDP"
    } else {
        "Ajouter un serveur RDP"
    };
    Html(page(
        title,
        &format!(
            r#"<h1>{title}</h1><p class="subtitle">Une modification sera utilisée par les kiosques associés lors de leur prochaine connexion RDP.</p>{}<form method="post" action="{action}" class="card"><fieldset><legend>Destination</legend><label>Nom du serveur<input name="name" type="text" value="{}" required></label><label>Adresse IP ou nom DNS<input name="address" type="text" value="{}" required></label><label>Port<input name="port" type="text" value="{}" required></label><label class="check"><input type="checkbox" name="ignore_certificate_errors" value="1"{}> Ignorer les erreurs de certificat</label></fieldset><div class="actions"><button class="btn primary">Enregistrer le serveur</button><a class="btn" href="/rdp-servers">Annuler</a></div></form>"#,
            error
                .map(|e| format!("<p class=\"error\">{}</p>", esc(e)))
                .unwrap_or_default(),
            esc(&form.name),
            esc(&form.address),
            esc(&form.port),
            if form.ignore_certificate_errors.is_some() {
                " checked"
            } else {
                ""
            }
        ),
    ))
}
pub async fn new_form() -> Html<String> {
    server_form(
        None,
        &ServerForm {
            port: "3389".into(),
            ..Default::default()
        },
        None,
    )
}
pub async fn create(State(state): State<Arc<AppState>>, Form(form): Form<ServerForm>) -> Response {
    match state.rdp_servers.save(None, &form) {
        Ok(_) => Redirect::to("/rdp-servers").into_response(),
        Err(e) => server_form(None, &form, Some(&e)).into_response(),
    }
}
pub async fn edit(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.rdp_servers.get(&id) {
        Some(s) => server_form(
            Some(&id),
            &ServerForm {
                name: s.name,
                address: s.address,
                port: s.port.to_string(),
                ignore_certificate_errors: s.ignore_certificate_errors.then(|| "1".into()),
            },
            None,
        )
        .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<ServerForm>,
) -> Response {
    match state.rdp_servers.save(Some(&id), &form) {
        Ok(_) => Redirect::to("/rdp-servers").into_response(),
        Err(e) => server_form(Some(&id), &form, Some(&e)).into_response(),
    }
}
pub async fn inline_create(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ServerForm>,
) -> Response {
    match state.rdp_servers.save(None, &form) {
        Ok(s) => Json(s).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":e})),
        )
            .into_response(),
    }
}
