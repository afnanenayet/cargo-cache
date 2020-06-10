// Copyright 2020 Matthias Krüger. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// remove all crates from a cache that are not referenced by a Cargo lockfile

use std::ffi::OsStr;
use std::path::PathBuf;

use crate::cache::caches::*;
use crate::cache::*;
use crate::library::{CargoCachePaths, Error};
use crate::remove::*;
use cargo_metadata::{CargoOpt, MetadataCommand};

// the source of a crate inside the cargo cache can be represented in form of
// an extracted .crate or a checked out git repository
// the path is the absolute path to the source inside the ${CARGO_HOME}
#[derive(Debug, Clone)]
enum SourceKind {
    Crate(PathBuf),
    Git(PathBuf),
}

// get the path contained in a SourceKind
impl SourceKind {
    fn inner(self) -> PathBuf {
        match self {
            SourceKind::Crate(p) => p,
            SourceKind::Git(p) => p,
        }
    }
}

fn find_crate_name_git(toml_path: &PathBuf, cargo_home: &PathBuf) -> SourceKind {
    //  ~/.cargo/registry/src/github.com-1ecc6299db9ec823/winapi-0.3.8/Cargo.toml => ~/.cargo/registry/src/github.com-1ecc6299db9ec823/winapi-0.3.8/

    // get the segments of the path
    let v: Vec<&OsStr> = toml_path.iter().collect();

    let checkouts_pos = v
        .iter()
        .position(|i| i == &"checkouts")
        .unwrap_or_else(|| panic!("failed to parse! 1: {:?}", v)); //@FIXME

    // assuming git:
    // git checkouts repo-name ref
    let path_segments = &v[(checkouts_pos - 1)..(checkouts_pos + 3)];

    let mut path = cargo_home.clone();
    path_segments.iter().for_each(|p| path.push(p));

    SourceKind::Git(path)
}

fn find_crate_name_crate(toml_path: &PathBuf, cargo_home: &PathBuf) -> SourceKind {
    // ~/.cargo/git/checkouts/home-fb9469891e5cfbe6/3a6eccd/cargo.toml  => ~/.cargo/git/checkouts/home-fb9469891e5cfbe6/3a6eccd/

    let v: Vec<&OsStr> = toml_path.iter().collect();

    let registry_pos = v
        .iter()
        .position(|i| i == &"registry")
        .unwrap_or_else(|| panic!("failed to parse! 2: {:?}", v)); //@FIXME

    let path_segments = &v[(registry_pos)..(registry_pos + 4)];
    let mut path = cargo_home.clone();
    path_segments.iter().for_each(|p| path.push(p));

    SourceKind::Crate(path)
}

pub(crate) fn clear_unref(
    cargo_cache_paths: &CargoCachePaths,
    checkouts_cache: &mut git_checkouts::GitCheckoutCache,
    bare_repos_cache: &mut git_repos_bare::GitRepoCache,
    registry_pkg_caches: &mut registry_pkg_cache::RegistryPkgCaches,
    registry_sources_caches: &mut registry_sources::RegistrySourceCaches,
    dry_run: bool,
    size_changed: &mut bool,
) -> Result<(), Error> {
    let cargo_home = &cargo_cache_paths.cargo_home;

    // get a list of all dependencies of the project
    let manifest = crate::local::get_manifest()?;

    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .features(CargoOpt::AllFeatures)
        .exec()
        .unwrap_or_else(|error| {
            panic!(
                //@FIXME
                "Failed to parse manifest: '{}'\nError: '{:?}'",
                &manifest.display(),
                error
            )
        });
    let dependencies = metadata.packages;

    // get the path inside the CARGO_HOME of the source of the dependency
    #[allow(clippy::filter_map)]
    let required_packages = dependencies
        .iter()
        .map(|pkg| &pkg.manifest_path)
        // we only care about tomls that are not local, i.e. tomls that are inside the $CARGO_HOME
        .filter(|toml_path| toml_path.starts_with(&cargo_home))
        // map the manifest paths to paths to the roots of the crates inside the cargo_home
        .map(|toml_path| {
            if toml_path.starts_with(&cargo_cache_paths.git_checkouts) {
                find_crate_name_git(toml_path, cargo_home)
            } else if toml_path.starts_with(&cargo_cache_paths.registry_sources) {
                find_crate_name_crate(toml_path, cargo_home)
            } else {
                // if we find a source path that is neither a git nor a crate dep, this probably indicates a bug
                unreachable!(
                    "ERROR: did not recognize toml path: '{}'",
                    toml_path.display()
                );
            }
        })
        // we need to map the git repo checkouts to bare git repos
        // and the source-checkouts to pkg cache archives!
        .map(|sourcekind| match sourcekind {
            SourceKind::Crate(registry_src_path) => {
                // ~/.cargo/registry/src/github.com-1ecc6299db9ec823/semver-0.9.0
                // =>
                // ~/.cargo/registry/cache/github.com-1ecc6299db9ec823/semver-0.9.0.crate
                let path = registry_src_path.iter().collect::<Vec<&OsStr>>();
                let package_name = path[path.len() - 1];
                let registry = path[path.len() - 2];
                let mut registry_cache_path = cargo_cache_paths.registry_pkg_cache.clone();
                // we need to push the registry index as well
                registry_cache_path.push(registry);
                // this can probably be
                // can't use .set_extension() here because "cratename-0.1.3" will detect the ".3" as extension
                // and change it
                registry_cache_path.push(format!(
                    "{}{}",
                    package_name.to_os_string().into_string().unwrap(),
                    ".crate"
                ));
                SourceKind::Crate(registry_cache_path)
            }
            SourceKind::Git(gitpath) => {
                // ~/.cargo/git/checkouts/cargo-e7ff1db891893a9e/258c896
                // =>
                // ~/.cargo/git/db/cargo-e7ff1db891893a9e
                let mut repo_name = gitpath;
                let _ = repo_name.pop(); // remove /258c896
                let repo_name = repo_name.iter().last().unwrap(); // cargo-e7ff1db891893a9e

                let mut db_name = cargo_cache_paths.git_repos_bare.clone();
                db_name.push(repo_name);
                // ~/.cargo/git/db/cargo-e7ff1db891893a9e
                SourceKind::Git(db_name)
            }
        });

    // now we have a list of all cargo-home-entries a crate needs to build
    // we can walk the cargo-cache and remove everything that is not referenced;
    // remove: git checkouts, registry sources
    // keep, if referenced: registry pkg cache, bare git repos

    // debug
    // println!("required packages:");
    // required_packages.inspect(|toml| println!("{:?}", toml));

    let remove = false;
    if remove {
        let dry_run = true;

        // remove the git checkout cache since it is not needed
        remove_file(
            &cargo_cache_paths.git_checkouts,
            dry_run,
            size_changed,
            None,
            None, // fixme
            Some(checkouts_cache.total_size()),
        );
        // invalidate cache
        let _ = &checkouts_cache.invalidate();

        // remove the registry_sources_cache as well
        remove_file(
            &cargo_cache_paths.registry_sources,
            dry_run,
            size_changed,
            None,
            None, // fixme
            Some(registry_sources_caches.total_size()),
        );
        // invalidate cache
        let _ = &registry_sources_caches.invalidate();

        let (required_crates, required_git_repos): (Vec<SourceKind>, Vec<SourceKind>) =
            required_packages.partition(|dep| match dep {
                SourceKind::Crate(_) => true,
                SourceKind::Git(_) => false,
            });

        // map SourceKinds to the contained paths
        let required_crates = required_crates
            .into_iter()
            .map(|sk| sk.inner())
            .collect::<Vec<PathBuf>>();
        let required_git_repos = required_git_repos
            .into_iter()
            .map(|sk| sk.inner())
            .collect::<Vec<PathBuf>>();
        // for the bare_repos_cache and registry_package_cache,
        // remove all items but the ones that are referenced

        let bare_repos = bare_repos_cache.bare_repo_folders();

        // all .crates found in the cache
        // @TODO add method to get all .crates of all caches via single method?
        let mut crates = Vec::new();

        for cache in registry_pkg_caches.caches() {
            crates.extend(cache.files());
        }

        // filter and remove git repos
        bare_repos
            .iter()
            .filter(|repo_in_cache| 
            // in the iterator, only keep crates that are not contained in
            // our dependency list and remove them

            !required_git_repos.contains(repo_in_cache))
            .for_each(|repo| { /* remove the repo */ });

        // filter and remove crate archives
        crates
            .iter()
            .filter(|crate_in_cache| 
            // in the iterator, only keep crates that are not contained in
            // our dependency list and remove them

            !required_crates.contains(crate_in_cache))
            .for_each(|repo| { /* remove the repo */ });
    }

    Ok(())
}