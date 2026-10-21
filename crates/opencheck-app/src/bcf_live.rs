//! BCF Live: issues shared with a BCF API 2.1 server (first Dalux).
//!
//! Sign-in uses the server's OAuth2 authorization-code flow (PKCE optional): the
//! browser opens the server's login page and returns to a small listener on
//! this computer. OpenCheck never sees the password. Tokens are kept per user
//! in `%APPDATA%\OpenCheck\bcf-servers.json`, never in project files.
//!
//! Every request runs on a background thread; results come back as
//! [`Message`]s that the UI applies with [`BcfLive::poll`].

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use opencheck_core::bcf::{
    self, AuthInfo, Comment, CommentRequest, CurrentUser, Extensions, Project, TokenResponse,
    Topic, TopicRequest, ViewpointRequest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The address Dalux gives its customers for BCF.
pub const DALUX_SERVER: &str = "https://node1.field.dalux.com/service/";
/// OAuth client id OpenCheck identifies itself with, unless the user sets one.
pub const DEFAULT_CLIENT_ID: &str = "opencheck";
/// Port of the sign-in listener on this computer.
pub const DEFAULT_REDIRECT_PORT: u16 = 51789;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// The same error is shown once within this time.
const REPEAT_ERROR_AFTER: Duration = Duration::from_secs(60);

/// A server the user has signed in to; kept with the user, not the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub server: String,
    #[serde(default = "default_client_id")]
    pub client_id: String,
    /// Only for servers that issue confidential clients; usually empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    #[serde(default = "default_port")]
    pub redirect_port: u16,
    /// Send a PKCE code challenge with the login. Dalux rejects it
    /// ("unknown arguments: code_challenge"), so it is off unless a server needs it.
    #[serde(default)]
    pub pkce: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// The access token and when it expires (Unix seconds). Kept so that a
    /// restart needs no new login while it is valid: Dalux gives no refresh token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub access_expires: u64,
    /// Name of the signed-in user, for display.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
}

fn default_client_id() -> String {
    DEFAULT_CLIENT_ID.to_owned()
}

fn default_port() -> u16 {
    DEFAULT_REDIRECT_PORT
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// When a token valid for `expires_in` seconds from `now` should be treated
/// as expired: a minute early, but never more than a tenth of its lifetime.
fn expiry(now: u64, expires_in: Option<u64>) -> u64 {
    let lifetime = expires_in.unwrap_or(3600);
    now + lifetime - (lifetime / 10).min(60)
}

impl Account {
    pub fn dalux() -> Self {
        Self {
            name: "Dalux".to_owned(),
            server: DALUX_SERVER.to_owned(),
            client_id: default_client_id(),
            client_secret: String::new(),
            redirect_port: DEFAULT_REDIRECT_PORT,
            pkce: false,
            refresh_token: None,
            access_token: None,
            access_expires: 0,
            user: String::new(),
        }
    }

    /// The access token, while it is still valid.
    fn valid_access_token(&self) -> Option<&str> {
        self.access_token
            .as_deref()
            .filter(|_| unix_now() < self.access_expires)
    }

    pub fn root(&self) -> String {
        bcf::api_root(&self.server)
    }

    fn redirect_uri(&self) -> String {
        format!("http://localhost:{}/callback", self.redirect_port)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedAccounts {
    accounts: Vec<Account>,
}

fn accounts_file() -> PathBuf {
    crate::library::folder()
        .parent()
        .map(|p| p.join("bcf-servers.json"))
        .unwrap_or_else(|| PathBuf::from("bcf-servers.json"))
}

/// `%APPDATA%/OpenCheck/bcf-log.txt`: every request to the BCF server with
/// its answer, for finding out why a server refuses something. Tokens and
/// passwords are never written.
pub fn log_file() -> PathBuf {
    accounts_file().with_file_name("bcf-log.txt")
}

fn log(line: &str) {
    use std::io::Write as _;
    let path = log_file();
    // Keep the log small: start over when it passes 1 MB.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{}  {line}", opencheck_core::project::now_rfc3339());
    }
}

fn load_accounts() -> Vec<Account> {
    std::fs::read_to_string(accounts_file())
        .ok()
        .and_then(|t| serde_json::from_str::<SavedAccounts>(&t).ok())
        .map(|s| s.accounts)
        .unwrap_or_default()
}

fn save_accounts(accounts: &[Account]) -> std::io::Result<()> {
    // Tests must not overwrite the user's sign-in.
    if cfg!(test) {
        return Ok(());
    }
    let path = accounts_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&SavedAccounts {
        accounts: accounts.to_vec(),
    })
    .expect("accounts always serialise");
    std::fs::write(path, text)
}

/// A topic ready to send, built from an issue on the UI thread.
pub struct Outgoing {
    pub issue: String,
    /// Existing topic to update; None creates one.
    pub topic_guid: Option<String>,
    pub topic: TopicRequest,
    /// Sent with a new topic.
    pub viewpoint: Option<ViewpointRequest>,
    /// The issue's `modified` time, to know later whether it changed again.
    pub version: String,
}

/// Results of background requests.
pub enum Message {
    SignedIn {
        server: String,
        tokens: TokenResponse,
    },
    SignInFailed(String),
    /// The sign-in can no longer be used (expired, no refresh token).
    Expired {
        server: String,
    },
    /// A refreshed access token (and maybe a new refresh token).
    Tokens {
        server: String,
        tokens: TokenResponse,
    },
    User(CurrentUser),
    Projects(Vec<Project>),
    Extensions {
        project: String,
        extensions: Extensions,
    },
    Sent {
        issue: String,
        version: String,
        topic: Topic,
    },
    Topics {
        project: String,
        topics: Vec<Topic>,
    },
    Comments {
        topic: String,
        comments: Vec<Comment>,
    },
    /// Sign-in finished; projects can be loaded.
    Ready,
    /// A status line for the user.
    Done(String),
    /// Something to report; the job may go on.
    Error(String),
    /// A background job ended, with its error if it failed.
    Finished(Result<(), String>),
}

#[derive(Clone)]
struct Credentials {
    root: String,
    server: String,
    client_id: String,
    client_secret: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

pub struct BcfLive {
    pub window_open: bool,
    pub accounts: Vec<Account>,
    /// Index into `accounts` of the server in use.
    pub current: usize,
    pub user: Option<CurrentUser>,
    pub projects: Vec<Project>,
    /// Values the linked project accepts (statuses, types, people).
    pub extensions: Option<Extensions>,
    pub extensions_for: String,
    /// Last topics fetched from the linked project.
    pub topics: Vec<Topic>,
    pub comments: Vec<(String, Vec<Comment>)>,
    pub status: String,
    pub signing_in: bool,
    /// Project whose value mapping was offered on its own, once.
    pub mapping_offered: String,
    busy: usize,
    last_error: Option<(String, Instant)>,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Default for BcfLive {
    fn default() -> Self {
        let (sender, receiver) = channel();
        let mut accounts = load_accounts();
        if accounts.is_empty() {
            accounts.push(Account::dalux());
        }
        Self {
            window_open: false,
            accounts,
            current: 0,
            user: None,
            projects: Vec::new(),
            extensions: None,
            extensions_for: String::new(),
            topics: Vec::new(),
            comments: Vec::new(),
            status: String::new(),
            signing_in: false,
            mapping_offered: String::new(),
            busy: 0,
            last_error: None,
            sender,
            receiver,
        }
    }
}

impl BcfLive {
    pub fn account(&self) -> &Account {
        &self.accounts[self.current.min(self.accounts.len() - 1)]
    }

    pub fn account_mut(&mut self) -> &mut Account {
        let i = self.current.min(self.accounts.len() - 1);
        &mut self.accounts[i]
    }

    pub fn save_accounts(&mut self) {
        if let Err(e) = save_accounts(&self.accounts) {
            self.status = format!("Could not save the server list: {e}");
        }
    }

    /// Signed in with a valid token, or able to get a new one silently.
    pub fn is_signed_in(&self) -> bool {
        let account = self.account();
        account.valid_access_token().is_some() || account.refresh_token.is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.busy > 0 || self.signing_in
    }

    /// People topics can be assigned to in the linked project.
    pub fn users(&self) -> &[String] {
        self.extensions
            .as_ref()
            .map_or(&[], |e| e.user_id_type.as_slice())
    }

    pub fn sign_out(&mut self) {
        self.user = None;
        self.projects.clear();
        self.extensions = None;
        let account = self.account_mut();
        account.refresh_token = None;
        account.access_token = None;
        account.access_expires = 0;
        account.user.clear();
        self.save_accounts();
        self.status = "Signed out.".to_owned();
    }

    fn credentials(&self) -> Credentials {
        let account = self.account();
        Credentials {
            root: account.root(),
            server: account.server.clone(),
            client_id: account.client_id.clone(),
            client_secret: account.client_secret.clone(),
            access_token: account.valid_access_token().map(str::to_owned),
            refresh_token: account.refresh_token.clone(),
        }
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&Client) -> Result<(), String> + Send + 'static,
    ) {
        self.busy += 1;
        let client = Client {
            credentials: std::sync::Mutex::new(self.credentials()),
            sender: self.sender.clone(),
            agent: agent(),
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = work(&client);
            let _ = client.sender.send(Message::Finished(result));
            ctx.request_repaint();
        });
    }

    /// Opens the browser at the server's login page and waits for it to come back.
    pub fn sign_in(&mut self, ctx: &egui::Context) {
        if self.signing_in {
            return;
        }
        self.signing_in = true;
        self.status = "Waiting for the login in the browser…".to_owned();
        let account = self.account().clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let message = match sign_in(&account) {
                Ok(tokens) => Message::SignedIn {
                    server: account.server.clone(),
                    tokens,
                },
                Err(e) => Message::SignInFailed(format!("Sign-in failed: {e}")),
            };
            let _ = sender.send(message);
            ctx.request_repaint();
        });
    }

    pub fn load_projects(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, |client| {
            let user: CurrentUser = client.get("/current-user").unwrap_or_default();
            client.send(Message::User(user));
            let projects: Vec<Project> = client.get("/projects")?;
            client.send(Message::Projects(projects));
            Ok(())
        });
    }

    pub fn load_extensions(&mut self, project: &str, ctx: &egui::Context) {
        let project = project.to_owned();
        let name = self
            .projects
            .iter()
            .find(|p| p.project_id == project && !p.name.is_empty())
            .map_or_else(|| project.clone(), |p| p.name.clone());
        self.spawn(ctx, move |client| {
            // Only the lists for the pickers; a failure leaves the sign-in alone.
            let extensions: Extensions = client
                .get(&format!("/projects/{project}/extensions"))
                .map_err(|e| {
                    format!(
                        "Could not read the statuses, types and people of the BCF project \"{name}\". \
                         Issues can still be sent; those fields are left to the server. ({e})"
                    )
                })?;
            client.send(Message::Extensions {
                project,
                extensions,
            });
            Ok(())
        });
    }

    /// Creates or updates topics; new ones get the issue's viewpoint.
    pub fn send(&mut self, project: &str, outgoing: Vec<Outgoing>, ctx: &egui::Context) {
        let project = project.to_owned();
        let count = outgoing.len();
        self.spawn(ctx, move |client| {
            for item in outgoing {
                let topic: Topic = match &item.topic_guid {
                    Some(guid) => {
                        client.put(&format!("/projects/{project}/topics/{guid}"), &item.topic)?
                    }
                    None => create_topic(client, &project, &item.topic)?,
                };
                if item.topic_guid.is_none()
                    && let Some(viewpoint) = &item.viewpoint
                {
                    let path = format!("/projects/{project}/topics/{}/viewpoints", topic.guid);
                    if let Err(e) = client.post::<serde_json::Value>(&path, viewpoint) {
                        client.send(Message::Error(format!(
                            "The topic was created, but its viewpoint was refused: {e}"
                        )));
                    }
                }
                client.send(Message::Sent {
                    issue: item.issue,
                    version: item.version,
                    topic,
                });
            }
            client.send(Message::Done(format!("{count} issues sent.")));
            Ok(())
        });
    }

    pub fn fetch_topics(&mut self, project: &str, ctx: &egui::Context) {
        let project = project.to_owned();
        self.spawn(ctx, move |client| {
            let topics: Vec<Topic> = client.get(&format!("/projects/{project}/topics"))?;
            client.send(Message::Topics { project, topics });
            Ok(())
        });
    }

    pub fn fetch_comments(&mut self, project: &str, topic: &str, ctx: &egui::Context) {
        let (project, topic) = (project.to_owned(), topic.to_owned());
        self.spawn(ctx, move |client| {
            let comments: Vec<Comment> =
                client.get(&format!("/projects/{project}/topics/{topic}/comments"))?;
            client.send(Message::Comments { topic, comments });
            Ok(())
        });
    }

    pub fn add_comment(&mut self, project: &str, topic: &str, text: String, ctx: &egui::Context) {
        let (project, topic) = (project.to_owned(), topic.to_owned());
        self.spawn(ctx, move |client| {
            let _: serde_json::Value = client.post(
                &format!("/projects/{project}/topics/{topic}/comments"),
                &CommentRequest { comment: text },
            )?;
            let comments: Vec<Comment> =
                client.get(&format!("/projects/{project}/topics/{topic}/comments"))?;
            client.send(Message::Comments { topic, comments });
            Ok(())
        });
    }

    /// Messages that arrived since the last frame. Token and list messages
    /// are applied here; the caller handles the ones about issues.
    pub fn poll(&mut self) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::SignedIn { server, tokens } => {
                    self.signing_in = false;
                    self.store_tokens(&server, tokens);
                    self.status = "Signed in.".to_owned();
                    out.push(Message::Ready);
                }
                Message::SignInFailed(text) => {
                    self.signing_in = false;
                    self.report(text, &mut out);
                }
                Message::Expired { server } => {
                    if self.account().server == server {
                        let account = self.account_mut();
                        account.access_token = None;
                        account.access_expires = 0;
                        self.save_accounts();
                    }
                }
                Message::Tokens { server, tokens } => self.store_tokens(&server, tokens),
                Message::User(user) => {
                    if !user.name.is_empty() || !user.id.is_empty() {
                        let name = if user.name.is_empty() {
                            user.id.clone()
                        } else {
                            user.name.clone()
                        };
                        if self.account().user != name {
                            self.account_mut().user = name;
                            self.save_accounts();
                        }
                    }
                    self.user = Some(user);
                }
                Message::Projects(projects) => self.projects = projects,
                Message::Extensions {
                    project,
                    extensions,
                } => {
                    self.extensions = Some(extensions);
                    self.extensions_for = project;
                }
                Message::Comments { topic, comments } => {
                    self.comments.retain(|(t, _)| *t != topic);
                    self.comments.push((topic, comments));
                }
                Message::Done(text) => self.status = text,
                Message::Error(text) => self.report(text, &mut out),
                Message::Finished(result) => {
                    self.busy = self.busy.saturating_sub(1);
                    if let Err(text) = result {
                        self.report(text, &mut out);
                    }
                }
                Message::Topics { project, topics } => {
                    self.topics = topics.clone();
                    out.push(Message::Topics { project, topics });
                }
                other => out.push(other),
            }
        }
        out
    }

    /// Shows an error in the status line and passes it on, but not the same
    /// one again within a minute.
    fn report(&mut self, text: String, out: &mut Vec<Message>) {
        self.status = text.clone();
        if let Some((last, at)) = &self.last_error
            && *last == text
            && at.elapsed() < REPEAT_ERROR_AFTER
        {
            return;
        }
        self.last_error = Some((text.clone(), Instant::now()));
        out.push(Message::Error(text));
    }

    fn store_tokens(&mut self, server: &str, tokens: TokenResponse) {
        if self.account().server != server {
            return;
        }
        let account = self.account_mut();
        if !tokens.access_token.is_empty() {
            account.access_expires = expiry(unix_now(), tokens.expires_in);
            account.access_token = Some(tokens.access_token);
        }
        if tokens.refresh_token.is_some() {
            account.refresh_token = tokens.refresh_token;
        }
        self.save_accounts();
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("OpenCheck/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Requests with a bearer token, refreshing it once when it has expired.
struct Client {
    credentials: std::sync::Mutex<Credentials>,
    sender: Sender<Message>,
    agent: ureq::Agent,
}

impl Client {
    fn send(&self, message: Message) {
        let _ = self.sender.send(message);
    }

    fn token(&self, force_refresh: bool) -> Result<String, String> {
        let mut c = self.credentials.lock().unwrap_or_else(|e| e.into_inner());
        if !force_refresh && let Some(token) = &c.access_token {
            return Ok(token.clone());
        }
        let Some(refresh) = c.refresh_token.clone() else {
            // Without a refresh token the login cannot be renewed silently.
            c.access_token = None;
            self.send(Message::Expired {
                server: c.server.clone(),
            });
            return Err(if force_refresh {
                "The server no longer accepts the sign-in. Press Authenticate to sign in again."
            } else {
                "The sign-in has expired. Press Authenticate to sign in again."
            }
            .to_owned());
        };
        let auth = auth_info(&self.agent, &c.root)?;
        let token_url = auth
            .oauth2_token_url
            .ok_or("The server gives no token address")?;
        let mut form = vec![
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh),
            ("client_id", c.client_id.clone()),
        ];
        if !c.client_secret.is_empty() {
            form.push(("client_secret", c.client_secret.clone()));
        }
        let tokens: TokenResponse = read(
            self.agent.post(&token_url).send_form(form),
            "refreshing the sign-in",
        )
        .map_err(|e| format!("{e} Sign in again with Authenticate."))?;
        c.access_token = Some(tokens.access_token.clone());
        if tokens.refresh_token.is_some() {
            c.refresh_token = tokens.refresh_token.clone();
        }
        self.send(Message::Tokens {
            server: c.server.clone(),
            tokens: tokens.clone(),
        });
        Ok(tokens.access_token)
    }

    fn root(&self) -> String {
        self.credentials
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .root
            .clone()
    }

    /// Runs a request with the current token; on 401 refreshes and retries once.
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        what: &str,
        request: impl Fn(&str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T, String> {
        let token = self.token(false)?;
        let response = request(&token).map_err(|e| {
            log(&format!("{what}: {e}"));
            format!("{what}: {e}")
        })?;
        log(&format!("{what} -> {}", response.status()));
        if response.status() == 401 {
            // Without a refresh token a 401 says only that the server refused
            // this request; the sign-in itself may still be good for others.
            let renewable = self
                .credentials
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .refresh_token
                .is_some();
            if !renewable {
                return read(Ok(response), what);
            }
            let token = self.token(true)?;
            return read(request(&token), what);
        }
        read(Ok(response), what)
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}{path}", self.root());
        self.call(&format!("GET {path}"), |token| {
            self.agent
                .get(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .call()
        })
    }

    fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, String> {
        log_body("POST", path, body);
        let url = format!("{}{path}", self.root());
        self.call(&format!("POST {path}"), |token| {
            self.agent
                .post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .send_json(body)
        })
    }

    fn put<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, String> {
        log_body("PUT", path, body);
        let url = format!("{}{path}", self.root());
        self.call(&format!("PUT {path}"), |token| {
            self.agent
                .put(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .send_json(body)
        })
    }
}

/// Reads a JSON answer, turning HTTP errors into readable messages.
fn read<T: serde::de::DeserializeOwned>(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    what: &str,
) -> Result<T, String> {
    let mut response = response.map_err(|e| format!("{what}: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        let body: String = body.chars().take(300).collect();
        log(&format!("{what}: {status} {body}"));
        return Err(match status.as_u16() {
            // Reading works with the same sign-in, so a refused write is a
            // question of rights, not of the login.
            401 if what.starts_with("POST") || what.starts_with("PUT") => format!(
                "{what}: the server accepted the sign-in but refused to write ({}). Your user may \
                 not have the right to create or edit BCF topics in this project, or the server \
                 only allows writing for registered client ids.",
                one_line(&body)
            ),
            401 => format!(
                "{what}: the server refused the sign-in for this request ({}). If it keeps \
                 happening, press Authenticate.",
                one_line(&body)
            ),
            403 => format!("{what}: no access ({body})."),
            _ => format!("{what}: the server answered {status} {body}"),
        });
    }
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{what}: unexpected answer: {e}"))?;
    // What the server says about projects and their settings helps when it
    // refuses something; these answers hold no secrets.
    if what.ends_with("/projects") || what.ends_with("/extensions") {
        log(&format!(
            "{what} answer: {}",
            body.chars().take(4000).collect::<String>()
        ));
    }
    serde_json::from_str::<T>(&body).map_err(|e| format!("{what}: unexpected answer: {e}"))
}

/// Writes what is sent, so a refusal can be traced to a field. Topic and
/// comment bodies hold no secrets.
fn log_body(method: &str, path: &str, body: &impl Serialize) {
    let text = serde_json::to_string(body).unwrap_or_default();
    log(&format!(
        "{method} {path} body: {}",
        text.chars().take(3000).collect::<String>()
    ));
}

/// Creates a topic. When the server refuses the full topic, it tries again
/// with only title, type and status (and then title and type), and sets the
/// other fields with an update; the log shows which form was accepted.
fn create_topic(client: &Client, project: &str, topic: &TopicRequest) -> Result<Topic, String> {
    let path = format!("/projects/{project}/topics");
    let first = match client.post::<Topic>(&path, topic) {
        Ok(created) => return Ok(created),
        Err(e) => e,
    };
    if !first.contains("refused") {
        return Err(first);
    }
    let reduced = [
        TopicRequest {
            title: topic.title.clone(),
            topic_type: topic.topic_type.clone(),
            topic_status: topic.topic_status.clone(),
            ..Default::default()
        },
        TopicRequest {
            title: topic.title.clone(),
            topic_type: topic.topic_type.clone(),
            ..Default::default()
        },
    ];
    for minimal in &reduced {
        log("retrying the topic with fewer fields");
        if let Ok(created) = client.post::<Topic>(&path, minimal) {
            log("the server accepted the topic with fewer fields; updating the rest");
            let update = format!("{path}/{}", created.guid);
            return match client.put::<Topic>(&update, topic) {
                Ok(updated) => Ok(updated),
                Err(e) => {
                    client.send(Message::Error(format!(
                        "The topic was created, but some fields were refused: {e}"
                    )));
                    Ok(created)
                }
            };
        }
    }
    Err(first)
}

/// A server answer on one line, e.g. `{ "message": "Unauthorized" }`.
fn one_line(body: &str) -> String {
    let text = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        "no details".to_owned()
    } else {
        text
    }
}

fn auth_info(agent: &ureq::Agent, root: &str) -> Result<AuthInfo, String> {
    read(
        agent.get(&format!("{root}/auth")).call(),
        "reading the login settings",
    )
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bcf::base64_url(&bytes))
}

/// The whole browser sign-in: authorization code (with PKCE when the server wants it) and a listener
/// on this computer for the way back.
fn sign_in(account: &Account) -> Result<TokenResponse, String> {
    let agent = agent();
    let auth = auth_info(&agent, &account.root())?;
    let (Some(auth_url), Some(token_url)) = (auth.oauth2_auth_url, auth.oauth2_token_url) else {
        return Err("the server does not offer an OAuth2 login".to_owned());
    };
    let verifier = random_token()?;
    let state = random_token()?;
    let redirect_uri = account.redirect_uri();

    // Listen on both loopback addresses: browsers resolve localhost either way.
    let listeners: Vec<TcpListener> = ["127.0.0.1", "[::1]"]
        .iter()
        .filter_map(|host| TcpListener::bind(format!("{host}:{}", account.redirect_port)).ok())
        .collect();
    if listeners.is_empty() {
        return Err(format!(
            "port {} on this computer is in use; choose another under Advanced",
            account.redirect_port
        ));
    }
    for l in &listeners {
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
    }

    let separator = if auth_url.contains('?') { '&' } else { '?' };
    let mut url = format!(
        "{auth_url}{separator}response_type=code&client_id={}&redirect_uri={}&state={}",
        bcf::url_encode(&account.client_id),
        bcf::url_encode(&redirect_uri),
        bcf::url_encode(&state),
    );
    if account.pkce {
        let challenge = bcf::base64_url(&Sha256::digest(verifier.as_bytes()));
        url.push_str(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            bcf::url_encode(&challenge)
        ));
    }
    webbrowser::open(&url).map_err(|e| format!("could not open the browser: {e}"))?;

    let started = Instant::now();
    let code = loop {
        if started.elapsed() > SIGN_IN_TIMEOUT {
            return Err("no answer from the browser within 5 minutes".to_owned());
        }
        let accepted = listeners.iter().find_map(|l| l.accept().ok());
        let Some((stream, _)) = accepted else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        match handle_callback(stream, &state) {
            Callback::Code(code) => break code,
            Callback::Denied(e) => return Err(e),
            Callback::Ignore => continue,
        }
    };

    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", account.client_id.clone()),
    ];
    if account.pkce {
        form.push(("code_verifier", verifier));
    }
    if !account.client_secret.is_empty() {
        form.push(("client_secret", account.client_secret.clone()));
    }
    let tokens: TokenResponse = read(
        agent.post(&token_url).send_form(form),
        "finishing the sign-in",
    )?;
    log(&format!(
        "signed in: access token {} characters, expires in {:?} s, refresh token {}",
        tokens.access_token.len(),
        tokens.expires_in,
        if tokens.refresh_token.is_some() {
            "yes"
        } else {
            "no"
        }
    ));
    Ok(tokens)
}

enum Callback {
    Code(String),
    Denied(String),
    /// Not the sign-in answer (e.g. a favicon request).
    Ignore,
}

fn handle_callback(stream: TcpStream, state: &str) -> Callback {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return Callback::Ignore;
    }
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params: Vec<(String, String)> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_owned(), bcf::url_decode(v)))
        .collect();
    let param = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    let (result, page) = if !path.starts_with("/callback") {
        (Callback::Ignore, None)
    } else if let Some(error) = param("error") {
        let detail = param("error_description").unwrap_or_default();
        (
            Callback::Denied(format!("the server refused the login: {error} {detail}")),
            Some("The login was refused. You can close this window and try again in OpenCheck."),
        )
    } else if param("state").as_deref() != Some(state) {
        (
            Callback::Ignore,
            Some("This answer does not belong to the login OpenCheck started."),
        )
    } else if let Some(code) = param("code") {
        (
            Callback::Code(code),
            Some("You are signed in. You can close this window and return to OpenCheck."),
        )
    } else {
        (Callback::Ignore, None)
    };
    let body = match page {
        Some(text) => format!(
            "<!doctype html><meta charset=utf-8><title>OpenCheck</title>\
             <body style=\"font-family:sans-serif;margin:3em\"><h2>OpenCheck</h2><p>{text}</p>"
        ),
        None => String::new(),
    };
    let status = if page.is_some() {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_expiry_keeps_short_lifetimes_usable() {
        assert_eq!(expiry(1000, Some(3600)), 1000 + 3540);
        assert_eq!(expiry(1000, Some(60)), 1000 + 54);
        assert_eq!(expiry(1000, Some(0)), 1000);
        assert_eq!(expiry(1000, None), 1000 + 3540);
    }

    #[test]
    fn an_error_is_reported_once() {
        let mut live = BcfLive::default();
        let mut out = Vec::new();
        live.report("GET /x: failed".to_owned(), &mut out);
        live.report("GET /x: failed".to_owned(), &mut out);
        live.report("GET /y: failed".to_owned(), &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn a_failed_extensions_call_keeps_the_sign_in() {
        let mut live = BcfLive {
            accounts: vec![Account::dalux()],
            ..BcfLive::default()
        };
        live.store_tokens(
            DALUX_SERVER,
            TokenResponse {
                access_token: "t".to_owned(),
                refresh_token: None,
                expires_in: Some(3600),
            },
        );
        live.busy = 1;
        let error = "GET /projects/p/extensions: unexpected answer".to_owned();
        live.sender.send(Message::Finished(Err(error))).unwrap();
        let out = live.poll();
        assert!(live.is_signed_in());
        assert!(!live.is_busy());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn callback_with_matching_state_gives_the_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            write!(
                s,
                "GET /callback?code=abc%2B1&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n"
            )
            .unwrap();
            let mut answer = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut answer);
            answer
        });
        let (stream, _) = listener.accept().unwrap();
        match handle_callback(stream, "xyz") {
            Callback::Code(code) => assert_eq!(code, "abc+1"),
            _ => panic!("expected a code"),
        }
        assert!(client.join().unwrap().contains("signed in"));
    }

    #[test]
    fn callback_with_wrong_state_is_ignored() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            write!(s, "GET /callback?code=abc&state=other HTTP/1.1\r\n\r\n").unwrap();
        });
        let (stream, _) = listener.accept().unwrap();
        assert!(matches!(handle_callback(stream, "xyz"), Callback::Ignore));
        client.join().unwrap();
    }
}
