# Releasing a tool

Salvage is a toolbox, and each tool releases on its own: its own tag, its own GitHub Release, and notes that only list changes to its own files. A tool can be part of the `salvage` command, or a separate thing with its own folder and build, such as a Mac app, an Unreal plugin or an editor add-on.

| Tool | Tag | Release title | Workflow |
| --- | --- | --- | --- |
| The `salvage` command | `cli-v<version>` | salvage command `<version>` | `.github/workflows/release-cli.yml` |

## How a release runs

1. A tool's own workflow, `release-<tool>.yml`, checks that the tag matches the version, builds the tool, and uploads the files to publish as artifacts named `<tool>-<anything>`.
2. It then calls the shared workflow, `release-publish.yml`. That workflow writes `SHA256SUMS`, writes the notes (an intro, then the commits since the tool's previous tag that touch the tool's paths), and publishes the GitHub Release.

Publishing only happens for a pushed tag with the tool's prefix. A manual run, or one from a pull request that changes the release tooling, is a dry run: everything is built and uploaded as workflow artifacts, and the notes appear in the run summary.

## Adding a tool

Say the tool is a Mac app in `apps/example/`, released from `example-v<version>` tags.

1. **Intro:** write `scripts/release/intros/example.md`. It's the top of every release's notes (what the tool is, how to install it, caveats such as signing). `{{version}}` is replaced with the version.
2. **Workflow:** add `.github/workflows/release-example.yml`:
   - Trigger on `push: tags: ['example-v*']` and `workflow_dispatch`.
   - Run a job that works out the version (however the tool stores it) and checks it with `scripts/release/check-tag.sh example-v <version>`.
   - Run a build job on whatever runner the tool needs. It should produce its archives or installers and upload them with `actions/upload-artifact`, named `example-<platform>`.
   - Run a `release` job with `permissions: contents: write` that calls the shared workflow:

     ```yaml
     uses: ./.github/workflows/release-publish.yml
     with:
       tool: example
       tag-prefix: example-v
       title: Salvage Example
       version: ${{ needs.version.outputs.version }}
       notes-intro: scripts/release/intros/example.md
       notes-paths: apps/example scripts/release/intros/example.md .github/workflows/release-example.yml
     ```

3. **Table:** add a row to the table above.

Nothing in `release-publish.yml` or the shared scripts needs to change. Pin every action by commit SHA, like the existing workflows.
