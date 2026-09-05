# AUTH.md — Microsoft auth + ownership gate (must implement before online play)

References: Prism Launcher `authentication` architecture, `minecraft-launcher-lib` docs,
`CCBlueX/minecraft-auth-java`, `ALinuxPerson/mcsoft-auth` (Rust), wiki.vg Microsoft Login tutorial.

## Flow (7 steps, OAuth2)

1. **MSA OAuth2** — browser or device-code flow. Scopes: `XboxLive.signin`, `XboxLive.offline_access`.
   Client ID: register your own Azure App (needs `api.minecraftservices.com` permission —
   request can take weeks/months). Dev placeholder: document clearly, never ship secret.
2. **Xbox user token**: `POST https://user.auth.xboxlive.com/user/authenticate` (RPS ticket).
3. **XSTS**: `POST https://xsts.auth.xboxlive.com/xsts/authorize` (relying party `rp://api.minecraftservices.com/`).
4. **Minecraft login**: `POST https://api.minecraftservices.com/authentication/login_with_xbox`
   (`identityToken: XBL3.0 x=<userhash>;<xsts>`). Returns MC access token + expires_in.
5. **Entitlements**: `GET https://api.minecraftservices.com/entitlements/mcstore`
   (or `/entitlements/license?requestId=<uuid>`) with `Authorization: Bearer <mc_token>`.
   Require `game_minecraft` or `product_minecraft`. Game Pass: entitlement may be empty but profile exists — handle explicitly.
6. **Profile**: `GET https://api.minecraftservices.com/minecraft/profile` → UUID, name, skins/capes.
7. **Session**: join server needs `sessionHash = SHA1(serverId + sharedSecret + publicKey)` POSTed to
   `sessionserver.mojang.com/session/minecraft/join`.

Token lifecycle: cache refresh token (0600 file or keychain), lazy refresh via `getUpToDate()` pattern.
Never log tokens. Device-code flow recommended for CLI/headless + 2FA safety.

## Rust crate (`mc-auth`) API sketch

```rust
// no secrets in Debug/Display
pub struct MinecraftSession { pub uuid: Uuid, pub username: String, pub access_token: RedactedString }
pub async fn login_device_code(client_id: &str) -> anyhow::Result<MinecraftSession>;
pub async fn refresh(session: &StoredTokens) -> anyhow::Result<MinecraftSession>;
pub async fn check_ownership(mc_token: &str) -> anyhow::Result<Ownership>; // game_minecraft/product_minecraft/profile
```

Tests: mock HTTP (wiremock), redaction tests, expiry/refresh tests. No live Microsoft calls in CI.

## AI rules

- Offline mode allowed for dev (`--offline <name>`) but online servers require ownership check pass.
- Document Azure App setup steps in PR if touching auth. Link EULA/Usage Guidelines.
