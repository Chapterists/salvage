# Deploying the site

The Salvage site is static: `npm run build` in `site/` writes plain files to `site/dist/`. A Cloudflare Worker serves them as static assets at **salvage.chapterists.com**. The Worker has no code of its own.

| File | What it does |
| --- | --- |
| `site/wrangler.jsonc` | The Worker: its name (`salvage-site`), the assets folder, the 404 page, and the custom domain. |
| `site/public/_headers` | Security headers and caching for every file served. |
| `site/astro.config.mjs` | The site URL used for canonical links, link previews and the sitemap, and the Content-Security-Policy each page carries. |
| `.github/workflows/deploy-site.yml` | Builds and deploys on every push to `main` that changes the site. |

Until the two secrets below exist, the workflow does nothing. It leaves a "Site not deployed" notice on the run and succeeds. Nothing is created in Cloudflare until then.

## 1. Make an API token

In the Cloudflare dashboard, go to **My Profile > API Tokens > Create Token** and start from the **Edit Cloudflare Workers** template. Then narrow it:

- **Permissions:** keep the template's Workers permissions. The deploy uploads a Worker and its static files, and needs nothing else, so don't add DNS, R2 or anything outside Workers.
- **Account resources:** include only the account that holds `chapterists.com`.
- **Zone resources:** include only `chapterists.com`.
- **TTL:** optional. A token that expires has to be replaced in GitHub when it does.

Copy the token. Cloudflare shows it only once.

The **account ID** is on the account's home page in the dashboard, and on the Workers & Pages overview.

## 2. Add the secrets to GitHub

In the repository, go to **Settings > Secrets and variables > Actions > New repository secret** and add:

| Name | Value |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | The token from step 1 |
| `CLOUDFLARE_ACCOUNT_ID` | The account ID |

Only the deploy workflow reads them, and only on pushes to `main` or a manual run. Pull requests never see them.

## 3. Deploy

Go to **Actions > Deploy site > Run workflow** on `main`. Later deploys happen by themselves whenever a push to `main` changes `site/`, `CONTRIBUTING.md` or `docs/benchmark-recipe.md` (the site renders those two).

The first deploy creates the `salvage-site` Worker.

## 4. Attach the domain

`site/wrangler.jsonc` asks for `salvage.chapterists.com` as a [custom domain](https://developers.cloudflare.com/workers/configuration/routing/custom-domains/). On the first deploy, Cloudflare creates the DNS record and certificate for it, as long as:

- `chapterists.com` is an active zone in the same Cloudflare account, and
- `salvage.chapterists.com` has no DNS record yet. A CNAME there blocks it, so delete any old one first.

Cloudflare's documentation doesn't say exactly which token permissions creating a custom domain needs. If the first deploy uploads the Worker but fails on the domain, attach it once by hand instead: **Workers & Pages > salvage-site > Settings > Domains & Routes > Add > Custom domain**, and enter `salvage.chapterists.com`. Later deploys leave an existing custom domain alone.

The `workers.dev` address and preview URLs are switched off in `wrangler.jsonc`, so the custom domain is the only address the site has.

To check it once it's live:

```sh
curl -sI https://salvage.chapterists.com/ | grep -iE '^(HTTP|content-security|strict-transport|x-frame)'
curl -s https://salvage.chapterists.com/robots.txt
curl -sI https://salvage.chapterists.com/no-such-page/ | head -1   # 404
```

## Rolling back

Every deploy is a version of the Worker, so going back is quick.

- **Dashboard:** **Workers & Pages > salvage-site > Deployments**. Pick an earlier version and roll back to it.
- **Command line,** with the same token in `CLOUDFLARE_API_TOKEN` and the account ID in `CLOUDFLARE_ACCOUNT_ID`:

  ```sh
  cd site
  npx wrangler@4.150.0 deployments list      # recent deployments and their versions
  npx wrangler@4.150.0 rollback <version-id> --message "Why"
  ```

A rollback only changes what's served. `main` still has the change, and the next push to the site deploys it again. To undo it for good, revert the commit on `main` and let the workflow redeploy.

To take the site down entirely, remove the custom domain from the Worker (step 4, in reverse), or delete the `salvage-site` Worker.

## Deploying by hand

You shouldn't need to, but with the same two environment variables set:

```sh
cd site
npm ci
npm run build
npx wrangler@4.150.0 deploy
```

`npx wrangler@4.150.0 deploy --dry-run` checks the configuration and lists the files it would upload, without an account.
