# Security

Nodestarr controls Docker, and anything that controls Docker controls the
machine: it can start a container that mounts the host's root file system.
Treat an admin account in Nodestarr like root on the server.

## Recommendations

- Keep Nodestarr on your local network, and reach it from outside through a
  VPN (WireGuard, Tailscale) rather than a public port.
- If you do publish it, put it behind a reverse proxy with HTTPS, and turn
  on two-step sign-in for every admin.
- Give people who only need to look a **viewer** account, and scripts and
  dashboards a **read-only API key**.
- Leave the local network bypass off unless everyone on your network
  should be an admin.

## Accounts and roles

- **Admins** can do everything.
- **Viewers** see containers, stacks, logs, metrics and history, but can't
  change anything, open consoles, or see secrets: container environment
  variables in edit forms, compose files and settings that hold passwords
  and tokens are admin only.

Manage users in Settings > Users. There is always at least one admin.

Passwords need at least 8 characters and are stored as Argon2id hashes.
Ten failed sign-ins from one address within 15 minutes lock that address out
for the rest of the window, and repeated failures can notify you
(`security.login_failed`).

## Two-step sign-in

Settings > Your account > Two-step sign-in sets up a TOTP authenticator app
(Aegis, 2FAS, Google Authenticator, 1Password, ...). You get ten one-time
recovery codes; keep them somewhere safe. A code can't be used twice.

Lost the authenticator and the codes? An admin with shell access can turn
it off: `docker exec nodestarr nodestarr disable-2fa --username NAME`.

## Sessions

Signing in creates a session cookie (`HttpOnly`, `SameSite=Lax`, and
`Secure` behind HTTPS) that lasts 24 hours, or 30 days with "keep me signed
in" (Settings > Sign-in and security). Settings > Your account lists where
you are signed in; sign out single sessions or everywhere else at once.
Changing your password also signs you out everywhere else.

Access is checked continuously: when a session is signed out, a user is
removed or loses the admin role, or an API key is deleted, open live
connections (the event stream, log streams and consoles) end immediately
and the browser shows the sign-in page.

## API keys

Settings > API keys creates keys with full or read-only access. A key
(`nsr_...`) is shown once and stored only as a hash. Send it as
`X-Api-Key: nsr_...` or `Authorization: Bearer nsr_...`. Keys in the URL
(`?apikey=nsr_...`, handy for tools that can't set headers) are accepted
unless you turn that off; URLs end up in logs, so prefer headers.

## Signing in through a reverse proxy

If an authenticating proxy (Authelia, Authentik, oauth2-proxy) already
signs people in, Nodestarr can trust it instead of showing its own sign-in
page. In Settings > Sign-in and security:

1. Add the proxy's address under **Trusted proxies**.
2. Choose **Reverse proxy (SSO)** under "Sign in with" and set the header
   that carries the user name (`Remote-User` by default).

Requests from trusted proxies with that header are signed in as admins
under that name; requests from anywhere else are refused. Make sure the
proxy is the only way to reach Nodestarr (bind Nodestarr to localhost or a
private network, or firewall the port), or anyone could send the header
themselves.

## Local network bypass

With **No sign-in from the local networks below** on (Settings > Sign-in
and security > Skip sign-in at home), requests from the listed networks
(private ranges, Tailscale's `100.64.0.0/10` and localhost by default) are
treated as admin without a password. Behind a reverse proxy, add the proxy
to trusted proxies so the real client address is used. Leave this off if
your network has guests or devices you don't fully trust.

## Protections in the web app

- Changes made with a session cookie must carry an `X-Nodestarr-Request`
  header, which other sites can't add without a CORS preflight Nodestarr
  never approves. WebSocket connections from other sites are refused.
- A strict Content Security Policy (no inline scripts), `nosniff`, and
  framing only by Nodestarr itself.
- Secrets (registry passwords, notification tokens, the Proxmox token, MQTT
  password) are never sent back to the browser; forms show "saved" instead.
  Settings exports leave them out unless you ask for them.
- Release notes and app messages are rendered from Markdown with all HTML
  escaped and only `http(s)` links.
- The stack editor reads and writes files only inside a stack's own folder
  and the stacks folders set in Settings > Docker (symlinks can't lead
  outside them), and only for admins.
- The database is readable only by its owner (it holds password hashes and
  API key hashes).

## Reporting a problem

If you find a security issue, please report it privately to the
maintainer rather than in a public issue.
