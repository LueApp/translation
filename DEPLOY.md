# Deploying the AI Translate site to Cloudflare

The landing page lives in [`site/`](site/) — a plain static site (HTML/CSS/JS, no
build step) plus the release download under [`site/downloads/`](site/downloads/).
Host it on Cloudflare at **`translate.lue-app.com`** (a subdomain of your
`lue-app.com` zone, the same pattern as `portscope.lue-app.com`).

```
site/
├── index.html                                  # the landing page
├── styles.css                                  # design
├── app.js                                       # language toggle + copy buttons
├── web2local-client.js                          # vendored web2local client library
├── web2local.js                                 # "install / run from the page" logic
├── _headers                                     # security + caching + download headers
└── downloads/
    ├── ai-translate-0.1.0-x86_64-linux.tar.gz          # the download (6.6 MB)
    ├── ai-translate-0.1.0-x86_64-linux.tar.gz.sha256   # checksum
    ├── install.sh                                       # standalone installer
    └── install-remote.sh                                # download+verify+install (used by web2local)
```

Every file is well under Cloudflare's 25 MiB per-asset limit, so the binary is
served straight from the deployment — no external file host needed.

## Prerequisites

- A Cloudflare account with the **`lue-app.com`** zone already added (its
  nameservers point at Cloudflare). Subdomain DNS + the TLS cert are then created
  for you automatically.
- Node.js (for `npx wrangler`). No install needed — `npx` fetches it.

Pick **one** of the two paths below. Both honor the `_headers` file.

---

## Path A — Wrangler (Workers static assets) · recommended

This repo already ships [`wrangler.toml`](wrangler.toml):

```toml
name = "ai-translate"
compatibility_date = "2026-05-01"

[assets]
directory = "./site"
```

Deploy:

```bash
cd /home/lue/boring/translation
npx wrangler login          # one-time, opens a browser to authorize
npx wrangler deploy
```

That publishes to `https://ai-translate.<your-subdomain>.workers.dev`. Now attach
the real domain:

1. Dashboard → **Workers & Pages** → **ai-translate** → **Settings** →
   **Domains & Routes** → **Add** → **Custom Domain**.
2. Enter **`translate.lue-app.com`** → **Add Domain**.

Because `lue-app.com` is on Cloudflare, the `CNAME` record and the edge
certificate are provisioned automatically (ready in seconds to a couple of
minutes).

> CLI alternative to step 1–2: nothing — custom domains for Workers are added in
> the dashboard (or via the API). The `wrangler deploy` above is all the CLI part.

---

## Path B — Cloudflare Pages

### B1. From the CLI

```bash
cd /home/lue/boring/translation
npx wrangler pages project create ai-translate --production-branch main   # one-time
npx wrangler pages deploy site --project-name ai-translate
```

### B2. From the dashboard (Git-connected, auto-deploys on push)

1. Push this repo to GitHub (`github.com/LueApp/translation`).
2. Dashboard → **Workers & Pages** → **Create** → **Pages** → **Connect to Git**
   → pick the repo.
3. Build settings: **Framework preset = None**, **Build command = (empty)**,
   **Build output directory = `site`**. Save & Deploy.

Either way, attach the domain:

- Pages project → **Custom domains** → **Set up a custom domain** →
  **`translate.lue-app.com`** → **Activate**. The DNS + cert are created for you.

---

## Verify

```bash
# page loads
curl -sI https://translate.lue-app.com/ | grep -i '200\|content-type'

# download is served with the right headers (attachment + immutable cache)
curl -sI https://translate.lue-app.com/downloads/ai-translate-0.1.0-x86_64-linux.tar.gz \
  | grep -iE 'content-type|content-disposition|cache-control'

# checksum matches what's on the page
curl -sL https://translate.lue-app.com/downloads/ai-translate-0.1.0-x86_64-linux.tar.gz -o /tmp/ait.tar.gz
curl -sL https://translate.lue-app.com/downloads/ai-translate-0.1.0-x86_64-linux.tar.gz.sha256
sha256sum /tmp/ait.tar.gz
# expected: 0c27209314c2d548083122dd4256be04ac28d0d6537eda3fd1477187e018bc66
```

Open the page, toggle **中文 / EN**, and click **Download for Linux** — it should
fetch the tarball directly.

---

## Shipping a new release

When you build a new binary, repackage and refresh the three references:

```bash
cd /home/lue/boring/translation
VER=0.2.0                      # new version
PKG="ai-translate-${VER}-x86_64-linux"

# stage + strip + package (mirror of how 0.1.0 was built)
rm -rf "/tmp/$PKG" && mkdir -p "/tmp/$PKG"
cp target/release/ai-translate "/tmp/$PKG/ai-translate" && strip "/tmp/$PKG/ai-translate"
cp site/downloads/install.sh "/tmp/$PKG/install.sh"
tar -czf "site/downloads/$PKG.tar.gz" -C /tmp "$PKG"
( cd site/downloads && sha256sum "$PKG.tar.gz" > "$PKG.tar.gz.sha256" )
```

Then update the version, filename, size, and sha256 in `site/index.html` (search
for the old version string and the old short sha shown in the hero), bump `VER`
in `site/downloads/install-remote.sh` (the web2local installer pins the version),
delete the old tarball, and redeploy with the same
`wrangler deploy` / `pages deploy` command. The versioned filename means old
links keep resolving and the `immutable` cache header is always correct.

## Notes

- **`_headers`** is honored by both Pages and Workers static assets. It forces
  `Content-Disposition: attachment` on `*.tar.gz` (so browsers download rather
  than try to render), sets `Content-Type: application/gzip`, and caches release
  artifacts for a year (`immutable`).
- The subdomain is just a convention — to use a different one (e.g.
  `ai-translate.lue-app.com`), change the Custom Domain in the dashboard and the
  `<link rel="canonical">` / `og:url` in `site/index.html`.
- `npx wrangler whoami` confirms which account you're deploying to.
