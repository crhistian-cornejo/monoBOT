# monoBOT update Worker

A Cloudflare Worker, in Rust compiled to WebAssembly, that serves monoBOT's
releases from the `monobot-releases` R2 bucket. The app checks it for updates
and the download page links to it. Nothing here needs GitHub Releases.

```
native-release.yml ──upload──> R2 monobot-releases
                                 ├─ latest.json             (signed feed)
                                 ├─ downloads.json          (installer per platform)
                                 └─ releases/<version>/…    (packages + .sig)
app updater ──GET /latest.json──────────> Worker ──> R2
download page ──GET /download/windows───> Worker ──302──> /releases/<version>/<installer>
```

## Routes

Only `GET` and `HEAD`; anything else is `405`.

| Path | Serves | Cache |
| --- | --- | --- |
| `/latest.json` | The signed update feed | `no-store` |
| `/downloads.json` (also `/download`) | `{ "version", "files": { key: file } }` | `no-store` |
| `/download/<key>` | `302` to the latest installer | `no-store` |
| `/releases/<version>/<file>` | A published file | one year, `immutable` |

Download keys: `windows`, `macos-arm64`, `macos-x64`, `linux-appimage`,
`linux-deb`, `linux-rpm`. Installers come with `Content-Disposition:
attachment`, and every JSON answer allows any origin, so a Pages site can read
the current version with `fetch`.

The Worker only reads the bucket. Paths are checked before they reach R2: a
version folder is digits, dots and dashes, a file name is letters, digits and
`._+-`, and `..`, hidden files and nested folders are refused.

## Deploy

```sh
cargo install worker-build
cd deploy/cloudflare-updates
cp wrangler.example.toml wrangler.toml
npx wrangler login
npx wrangler deploy
```

`wrangler deploy` prints the Worker's URL
(`https://monobot-updates.<subdomain>.workers.dev`). Store it as the
`MONOBOT_UPDATES_URL` repository secret: the release workflow builds it into
the app as the update feed and writes it into `latest.json`'s package URLs.

To serve it from your Pages domain later, uncomment `routes` in
`wrangler.toml` and deploy again. Then change `MONOBOT_UPDATES_URL`; apps
already installed keep checking the old URL, so keep `workers_dev = true`.

## Download page

Link the buttons to `<MONOBOT_UPDATES_URL>/download/<key>`. To show the
version:

```js
const { version } = await fetch("https://monobot-updates.example.workers.dev/downloads.json")
  .then((r) => r.json());
```

## Tests

```sh
cargo test                                        # routing and checks
cargo check --target wasm32-unknown-unknown       # the Cloudflare binding
```
