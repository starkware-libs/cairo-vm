# Release Process

## Typical workflow

- [ ] Pull latest from `main` branch.
      `git checkout main && git pull origin main`
- [ ] Determine release version string dependending on whether changes included
      in changelog are API breaking, it's a release candidate, etc.
      The release string should have the format "vX.Y.Z", with a possible
      trailing "-rcN" and follow [semantic versioning](https://semver.org/).
- [ ] Checkout branch named `release-N` where N is the version string.
      `git checkout -b release-N`
- [ ] Update the version field in the package entry of `Cargo.toml` files.
  - The versions must be the same.
  - You need to update the workspace dependency `cairo-vm`, which
    you can find in the root cargo manifest under the section `[workspace.dependencies]`.
  - [Here](https://github.com/starkware-libs/cairo-vm/pull/1748/files) is an
    example pull request with these changes.
- [ ] Run `cargo update` and `git add Cargo.lock`
- [ ] Update `CHANGELOG.md`:
  - Verify that the changelog is up to date.
  - Add a title with the release version string just below the `Upcoming
    Changes` section.
- [ ] Commit your changes, push your branch, and create a pull request.
- [ ] Merge after CI and review passes.
- [ ] Pull latest from `main` again.
- [ ] Tag commit with version string and push tag.
      `git tag -a <version string> -m "Release..."`
- [ ] Watch the `publish` workflow run in Github Actions.
- [ ] Verify all the crates are available on crates.io with the correct
      versions.
  - [cairo-vm](https://crates.io/crates/cairo-vm)
- [ ] Create a release in Github.
  - Select the recently created tag.
  - Set the title to the version string.
  - If it is a release candidate, mark it as a draft release.
- [ ] Announce release through corresponding channels.

## Hotfix releases

Sometimes there's a critical bug in a released version and we made breaking
changes in `main` since. To push a non-breaking hotfix, you need to  follow the
original steps but basing using the release branch you created previously as
base and bumping the patch level.

## Retroactive releases

In the case we want to push a new release from a revision other than `HEAD`
(say, we introduced features that we'd rather not ship yet), there would be no
base commit to compare to for the normal GitHub PR, as it can only merge
branches. This means we lose the review instance before publishing.
A solution is branching twice, once to diverge from main and one for making the
needed changes and creating a PR against the original commit.

