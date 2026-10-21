# BCF Live (Dalux)

OpenCheck shares issues with a BCF server through the buildingSMART BCF API
2.1. The first server is Dalux.

## Responsibility

Every issue has:

- **Responsible disciplines.** When an issue is created from findings, it
  gets the disciplines of the models the elements come from, e.g. a duct
  clashing with a beam is the responsibility of VEN and STR. Click a
  discipline in the issue to add or remove it.
- **Assigned to**: one person on the BCF server (BCF topics have one
  assignee). Pick from the people the Dalux project lists.

In the BCF Live window you can name a default person per discipline. An issue
without its own assignee is assigned to the person of its first responsible
discipline. All responsible disciplines are written in the topic's
description.

## Connecting

1. Issues tab → **Connect to Dalux…** (or click the server name later).
2. Check the **Url**: the BCF address Dalux gives you, for example
   `https://node1.field.dalux.com/service/`.
3. **Authenticate** opens Dalux's login page in your browser. Log in there;
   the browser then returns to OpenCheck on `http://localhost:51789/callback`.
   OpenCheck never sees your password.
4. Choose the **project**. Its people and statuses are loaded.

The sign-in (the access token with its expiry, and a refresh token when the
server gives one) is kept for your Windows user in
`%APPDATA%\OpenCheck\bcf-servers.json`, never in the `.opencheck` file, so a
restart needs no new login while the token is valid. The project file only
remembers which BCF project its issues belong to.

When the server gives no refresh token (Dalux did not in the first test) and the access token expires OpenCheck
shows **Authenticate** again instead of Send and Fetch. If the project's
statuses, types and people cannot be read, issues can still be sent; the
error is shown once and the sign-in is kept.

Under **Advanced** you can set the OAuth client id, client secret, login port
and PKCE (off by default, since Dalux rejects the code challenge). The
OAuth client id is `opencheck` unless you set another; if
Dalux requires a registered client, ask Dalux for a client id for OpenCheck
with the redirect address `http://localhost:51789/callback`.

## Value mapping

Servers name their values their own way. Dalux, for example, offers the
statuses "Aktiv", "Løst", "Lukkede" and "Afventer", priorities "Høj", "Mellem",
"Lav" and "C1"–"C3", and people as "name@company.dk | Role". As in Solibri,
the BCF Live window opens a **value mapping** the first time a project's
values are loaded: pair each OpenCheck status, criticality (C1–C3), the topic
type, the phase (BCF stage) and each person used in the issues (initials) with
the server's value, and press **Save mapping**. *Automatic* matches by name;
initials also match an e-mail address that starts with them (`ABC` →
`abc@example.com | …`). The mapping is saved in the project, so everyone who
opens it sends the same values. Fetched statuses and assignees are mapped
back the same way.

The window also shows the rights the server gives your user in the project.
Dalux lists them with the project's values; without `createTopic`, new issues
are not sent (issues already on the server can still be updated).

## Sending and fetching

- **Send changes** creates a topic for every new issue and updates the topic
  of every issue changed since it was last sent. With issues ticked, only
  those are sent. New topics get the issue's viewpoint: camera, section
  plane, the components selected, and which elements are visible.
- **Fetch** reads the project's topics. Status and assignee changes made on
  the server are taken over into the issues.
- **Comments on the server** in an issue loads its comments and posts new
  ones.

Statuses are matched by name to what the server offers ("Open", "Åben",
"Closed", "Lukket", ...). Topic type and priority are picked the same way;
when the server has no matching value the field is left to the server's
default.

## Not yet

Snapshots (images) are not sent with viewpoints, viewpoints are not updated
after the first send, and only Dalux has been targeted.
