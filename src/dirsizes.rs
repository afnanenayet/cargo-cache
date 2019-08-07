// Copyright 2017-2019 Matthias Krüger. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// This file provides the `DirSize` struct which holds information on the sizes and the number of files of the cargo cache.
/// When constructing the struct, the caches from the cache modules are used.
/// The new() method does parallel processing to a bit of time
use std::fmt;

use crate::cache::dircache::Cache;
use crate::cache::dircache::RegistrySubCache;
use crate::cache::dircache::RegistrySuperCache;

use crate::cache::*;
use crate::display::*;
use crate::library::*;

use humansize::{file_size_opts, FileSize};

/// Holds the sizes and the number of files of the components of the cargo cache
#[derive(Debug)]
pub(crate) struct DirSizes<'a> {
    /// total size of the cache / .cargo rood directory
    pub(crate) total_size: u64,
    /// number of binaries found
    pub(crate) numb_bins: usize,
    /// total size of binaries
    pub(crate) total_bin_size: u64,
    /// total size of the registries (src + cache)
    pub(crate) total_reg_size: u64,
    /// total size of the git db (bare repos and checkouts)
    pub(crate) total_git_db_size: u64,
    /// total size of bare git repos
    pub(crate) total_git_repos_bare_size: u64,
    /// number of bare git repos
    pub(crate) numb_git_repos_bare_repos: usize,
    /// number of git checkouts (source checkouts)
    pub(crate) numb_git_checkouts: usize,
    /// total size of git checkouts
    pub(crate) total_git_chk_size: u64,
    /// total size of registry caches (.crates)
    pub(crate) total_reg_cache_size: u64,
    /// total size of registry sources (extracted .crates, .rs sourcefiles)
    pub(crate) total_reg_src_size: u64,
    /// total size of registry indices
    pub(crate) total_reg_index_size: u64,
    /// total number of registry indices
    pub(crate) total_reg_index_num: u64,
    /// number of source archives (.crates) // @TODO clarify
    pub(crate) numb_reg_cache_entries: usize,
    /// number of registry source checkouts// @TODO clarify
    pub(crate) numb_reg_src_checkouts: usize,
    /// root path of the cache
    pub(crate) root_path: &'a std::path::PathBuf,
}

impl<'a> DirSizes<'a> {
    /// create a new DirSize object by querying the caches for their data, done in parallel
    pub(crate) fn new(
        bin_cache: &mut bin::BinaryCache,
        checkouts_cache: &mut git_checkouts::GitCheckoutCache,
        bare_repos_cache: &mut git_repos_bare::GitRepoCache,
        registry_pkg_cache: &mut registry_pkg_cache::RegistryPkgCaches,
        registry_index_caches: &mut registry_index::RegistryIndicesCache,
        registry_sources_caches: &mut registry_sources::RegistrySourceCaches,
        ccd: &'a CargoCachePaths,
    ) -> Self {
        // @TODO this is a mess and there's probably a way cleaner way to do this (threadpool?)
        #[allow(clippy::type_complexity)]
        let (
            (
                reg_index_size,
                ((bin_dir_size, numb_bins), (total_git_repos_bare_size, numb_git_repos_bare_repos)),
            ),
            (
                (total_git_chk_size, numb_git_checkouts),
                (
                    (total_reg_cache_size, total_reg_cache_entries),
                    (total_reg_src_size, numb_reg_src_checkouts),
                ),
            ),
        ): (
            (u64, ((u64, usize), (u64, usize))),
            ((u64, usize), ((u64, usize), (u64, usize))),
        ) = rayon::join(
            || {
                rayon::join(
                    || registry_index_caches.total_size(),
                    || {
                        rayon::join(
                            || (bin_cache.total_size(), bin_cache.number_of_files()),
                            || {
                                (
                                    bare_repos_cache.total_size(),
                                    bare_repos_cache.number_of_checkout_repos().unwrap(),
                                )
                            },
                        )
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        (
                            checkouts_cache.total_size(),
                            checkouts_cache.number_of_files_at_depth_2(),
                        )
                    },
                    || {
                        rayon::join(
                            || {
                                (
                                    registry_pkg_cache.total_size(),
                                    registry_pkg_cache.total_number_of_files(),
                                )
                            },
                            || {
                                (
                                    registry_sources_caches.total_size(),
                                    registry_sources_caches
                                        .total_number_of_source_checkout_folders(),
                                )
                            },
                        )
                    },
                )
            },
        );

        let root_path = &ccd.cargo_home;
        let total_reg_size = total_reg_cache_size + total_reg_src_size + reg_index_size;
        let total_git_db_size = total_git_repos_bare_size + total_git_chk_size;

        let total_bin_size = bin_dir_size;

        let total_size = total_reg_size + total_git_db_size + total_bin_size;
        Self {
            total_size,                           // total size of cargo root dir
            numb_bins,                            // number of binaries found
            total_bin_size,                       // total size of binaries found
            total_reg_size,                       // registry size
            total_git_db_size,                    // size of bare repos and checkouts combined
            total_git_repos_bare_size,            // git db size
            numb_git_repos_bare_repos,            // number of cloned repos
            numb_git_checkouts,                   // number of checked out repos
            total_git_chk_size,                   // git checkout size
            total_reg_cache_size,                 // registry cache size
            total_reg_src_size,                   // registry sources size
            total_reg_index_size: reg_index_size, // registry index size
            total_reg_index_num: registry_index_caches.number_of_items() as u64, // number  of indices //@TODO parallelize like the rest
            numb_reg_cache_entries: total_reg_cache_entries, // number of source archives
            numb_reg_src_checkouts,                          // number of source checkouts
            root_path,
        }
    }
}

impl<'a> DirSizes<'a> {
    /// returns the header of the summary which contains the path to the cache and its total size
    fn header(&self) -> Vec<TableLine> {
        vec![
            TableLine::new(
                0,
                format!("Cargo cache '{}':\n\n", &self.root_path.display()),
                String::new(),
            ),
            TableLine::new(
                0,
                "Total: ".to_string(),
                self.total_size.file_size(file_size_opts::DECIMAL).unwrap(),
            ),
        ]
    }

    /// returns amount and size of installed crate binaries
    fn bin(&self) -> Vec<TableLine> {
        vec![TableLine::new(
            1,
            format!("{} installed binaries: ", self.numb_bins),
            self.total_bin_size
                .file_size(file_size_opts::DECIMAL)
                .unwrap(),
        )]
    }

    /// returns amount and size of bare git repos and git repo checkouts
    fn git(&self) -> Vec<TableLine> {
        vec![
            TableLine::new(
                1,
                "Git db: ".to_string(),
                self.total_git_db_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
            TableLine::new(
                2,
                format!("{} bare git repos: ", self.numb_git_repos_bare_repos),
                self.total_git_repos_bare_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
            TableLine::new(
                2,
                format!("{} git repo checkouts: ", self.numb_git_checkouts),
                self.total_git_chk_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
        ]
    }

    /// returns summary of sizes of registry indices and registries (both, .crate archives and the extracted sources)
    fn registries_summary(&self) -> Vec<TableLine> {
        vec![
            TableLine::new(
                1,
                "Registry: ".to_string(),
                self.total_reg_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
            TableLine::new(
                2,
                // check how many indices there are
                match self.total_reg_index_num {
                    1 => String::from("Registry index: "),
                    _ => format!("{} registry indices: ", &self.total_reg_index_num),
                },
                self.total_reg_index_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
            TableLine::new(
                2,
                format!("{} crate archives: ", self.numb_reg_cache_entries),
                self.total_reg_cache_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
            TableLine::new(
                2,
                format!("{} crate source checkouts: ", self.numb_reg_src_checkouts),
                self.total_reg_src_size
                    .file_size(file_size_opts::DECIMAL)
                    .unwrap(),
            ),
        ]
    }

    /// returns more detailed summary about each registry
    fn registries_seperate(
        &self,
        index_caches: &mut registry_index::RegistryIndicesCache,
        registry_sources: &mut registry_sources::RegistrySourceCaches,
        pkg_caches: &mut registry_pkg_cache::RegistryPkgCaches,
    ) -> Vec<TableLine> {
        let mut v: Vec<TableLine> = vec![];

        // we need to match the separate registries together somehow
        // do this by folder names
        let mut registries: Vec<String> = vec![];
        index_caches.caches().iter().for_each(|registry| {
            registries.push(
                registry
                    .path()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            )
        });

        pkg_caches.caches().iter().for_each(|registry| {
            registries.push(
                registry
                    .path()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            )
        });

        registry_sources.caches().iter().for_each(|registry| {
            registries.push(
                registry
                    .path()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            )
        });
        // we now collected all the folder names of the registries and can match a single registry across multiple
        // caches by this

        /*
          Registry:                         1.52 GB
            5 registry indices:           250.20 MB
            5399 crate archives:          805.46 MB
            901 crate source checkouts:   460.77 MB
        */

        registries.sort();
        registries.dedup();

        for registry in &registries {
            let mut total_size = 0;

            let mut temp_vec: Vec<TableLine> = Vec::new();
            let mut registry_name: Option<String> = None;

            for index in index_caches.caches().iter_mut().filter(|r| {
                &r.path().file_name().unwrap().to_str().unwrap().to_string() == registry
            }) {
                temp_vec.push(TableLine::new(
                    2,
                    String::from("Registry index:"),
                    index
                        .total_size()
                        .file_size(file_size_opts::DECIMAL)
                        .unwrap(),
                ));
                total_size += index.total_size();
                if registry_name.is_none() {
                    registry_name = Some(index.name().into());
                }
            }

            for pkg_cache in pkg_caches.caches().iter_mut().filter(|p| {
                &p.path().file_name().unwrap().to_str().unwrap().to_string() == registry
            }) {
                temp_vec.push(TableLine::new(
                    2,
                    format!("{} crate archives: ", pkg_cache.number_of_files()),
                    pkg_cache
                        .total_size()
                        .file_size(file_size_opts::DECIMAL)
                        .unwrap(),
                ));
                total_size += pkg_cache.total_size();
                if registry_name.is_none() {
                    registry_name = Some(pkg_cache.name().into());
                }
            }

            for registry_source in registry_sources.caches().iter_mut().filter(|s| {
                &s.path().file_name().unwrap().to_str().unwrap().to_string() == registry
            }) {
                temp_vec.push(TableLine::new(
                    2,
                    format!(
                        "{} crate source checkouts: ",
                        registry_source.number_of_source_checkout_folders()
                    ),
                    registry_source
                        .total_size()
                        .file_size(file_size_opts::DECIMAL)
                        .unwrap(),
                ));
                total_size += registry_source.total_size();
                if registry_name.is_none() {
                    registry_name = Some(registry_source.name().into());
                }
            }

            let header_line = TableLine::new(
                1,
                format!("Registry: {}", registry_name.unwrap_or_default()),
                total_size.file_size(file_size_opts::DECIMAL).unwrap(),
            );

            v.push(header_line);
            v.extend(temp_vec);
        }

        v
    }
}

impl<'a> fmt::Display for DirSizes<'a> {
    /// returns the default summary of cargo-cache (cmd: "cargo cache")
    fn fmt(&self, f: &'_ mut fmt::Formatter<'_>) -> fmt::Result {
        let mut table: Vec<TableLine> = vec![];
        table.extend(self.header());
        table.extend(self.bin());
        table.extend(self.registries_summary());
        table.extend(self.git());

        let string: String = format_2_row_table(2, table, false);

        write!(f, "{}", string)?;
        Ok(())
    }
}

/// returns a summary with details on each registry (cmd: "cargo cache registry")
pub(crate) fn per_registry_summary(
    dir_size: &DirSizes<'_>,
    mut index_caches: &mut registry_index::RegistryIndicesCache,
    mut pkg_caches: &mut registry_sources::RegistrySourceCaches,
    mut registry_sources: &mut registry_pkg_cache::RegistryPkgCaches,
) -> String {
    let mut table: Vec<TableLine> = vec![];
    table.extend(dir_size.header());
    table.extend(dir_size.bin());
    table.extend(dir_size.registries_seperate(
        &mut index_caches,
        &mut pkg_caches,
        &mut registry_sources,
    ));
    table.extend(dir_size.git());

    format_2_row_table(2, table, false)
}

#[cfg(test)]
mod libtests {
    use super::*;

    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    impl<'a> DirSizes<'a> {
        #[allow(clippy::cast_possible_truncation)]
        #[allow(non_snake_case)]
        pub(super) fn new_manually(
            DI_bindir: &DirInfo,
            DI_git_repos_bare: &DirInfo,
            DI_git_checkout: &DirInfo,
            DI_reg_cache: &DirInfo,
            DI_reg_src: &DirInfo,
            DI_reg_index: &DirInfo,
            path: &'a PathBuf,
        ) -> Self {
            let bindir = DI_bindir;
            let git_repos_bare = DI_git_repos_bare;
            let git_checkouts = DI_git_checkout;
            let reg_cache = DI_reg_cache;
            let reg_src = DI_reg_src;
            let reg_index = DI_reg_index;

            let total_reg_size = reg_index.dir_size + reg_cache.dir_size + reg_src.dir_size;
            let total_git_db_size = git_repos_bare.dir_size + git_checkouts.dir_size;

            Self {
                // no need to recompute all of this from scratch
                total_size: total_reg_size + total_git_db_size + bindir.dir_size,
                numb_bins: bindir.file_number as usize,
                total_bin_size: bindir.dir_size,
                total_reg_size,

                total_git_db_size,
                total_git_repos_bare_size: git_repos_bare.dir_size,
                numb_git_repos_bare_repos: git_repos_bare.file_number as usize,

                total_git_chk_size: git_checkouts.dir_size,
                numb_git_checkouts: git_checkouts.file_number as usize,

                total_reg_cache_size: reg_cache.dir_size,
                numb_reg_cache_entries: reg_cache.file_number as usize,

                total_reg_src_size: reg_src.dir_size,
                numb_reg_src_checkouts: reg_src.file_number as usize,

                total_reg_index_size: reg_index.dir_size,
                total_reg_index_num: 1,
                root_path: path,
            }
        }
    }

    #[allow(non_snake_case)]
    #[test]
    fn test_DirSizes() {
        // DirInfors to construct DirSizes from
        let bindir = DirInfo {
            dir_size: 121_212,
            file_number: 31,
        };
        let git_repos_bare = DirInfo {
            dir_size: 121_212,
            file_number: 37,
        };
        let git_checkouts = DirInfo {
            dir_size: 34984,
            file_number: 8,
        };
        let reg_cache = DirInfo {
            dir_size: 89,
            file_number: 23445,
        };
        let reg_src = DirInfo {
            dir_size: 1_938_493_989,
            file_number: 123_909_849,
        };
        let reg_index = DirInfo {
            dir_size: 23,
            file_number: 12345,
        };

        let pb = PathBuf::from("/home/user/.cargo");

        // create a DirSizes object
        let dirSizes = DirSizes::new_manually(
            &bindir,
            &git_repos_bare,
            &git_checkouts,
            &reg_cache,
            &reg_src,
            &reg_index,
            &pb,
        );

        let output_is = format!("{}", dirSizes);

        let output_should = "Cargo cache '/home/user/.cargo':

Total:                                    1.94 GB
  31 installed binaries:                121.21 KB
  Registry:                               1.94 GB
    Registry index:                         23  B
    23445 crate archives:                   89  B
    123909849 crate source checkouts:     1.94 GB
  Git db:                               156.20 KB
    37 bare git repos:                  121.21 KB
    8 git repo checkouts:                34.98 KB\n";

        assert_eq!(output_is, output_should);
    }

    #[allow(non_snake_case)]
    #[test]
    fn test_DirSizes_gigs() {
        // DirInfors to construct DirSizes from
        let bindir = DirInfo {
            dir_size: 6_4015_8118,
            file_number: 69,
        };
        let git_repos_bare = DirInfo {
            dir_size: 3_0961_3689,
            file_number: 123,
        };
        let git_checkouts = DirInfo {
            dir_size: 39_2270_2821,
            file_number: 36,
        };
        let reg_cache = DirInfo {
            dir_size: 5_5085_5781,
            file_number: 3654,
        };
        let reg_src = DirInfo {
            dir_size: 9_0559_6846,
            file_number: 1615,
        };
        let reg_index = DirInfo {
            dir_size: 23,
            file_number: 0,
        };

        let pb = PathBuf::from("/home/user/.cargo");
        // create a DirSizes object
        let dirSizes = DirSizes::new_manually(
            &bindir,
            &git_repos_bare,
            &git_checkouts,
            &reg_cache,
            &reg_src,
            &reg_index,
            &pb,
        );

        let output_is = format!("{}", dirSizes);

        let output_should = "Cargo cache '/home/user/.cargo':

Total:                               6.33 GB
  69 installed binaries:           640.16 MB
  Registry:                          1.46 GB
    Registry index:                    23  B
    3654 crate archives:           550.86 MB
    1615 crate source checkouts:   905.60 MB
  Git db:                            4.23 GB
    123 bare git repos:            309.61 MB
    36 git repo checkouts:           3.92 GB\n";

        assert_eq!(output_is, output_should);
    }

    #[allow(non_snake_case)]
    #[test]
    fn test_DirSizes_almost_empty() {
        // DirInfors to construct DirSizes from
        let bindir = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let git_repos_bare = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let git_checkouts = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let reg_cache = DirInfo {
            dir_size: 130_4234_1234,
            file_number: 4,
        };
        let reg_src = DirInfo {
            dir_size: 2_6846_1234,
            file_number: 4,
        };
        let reg_index = DirInfo {
            dir_size: 12_5500_0000,
            file_number: 1,
        };

        let pb = PathBuf::from("/home/user/.cargo");

        // create a DirSizes object
        let dirSizes = DirSizes::new_manually(
            &bindir,
            &git_repos_bare,
            &git_checkouts,
            &reg_cache,
            &reg_src,
            &reg_index,
            &pb,
        );

        let output_is = format!("{}", dirSizes);

        let output_should = "Cargo cache '/home/user/.cargo':

Total:                           14.57 GB
  0 installed binaries:              0  B
  Registry:                      14.57 GB
    Registry index:               1.25 GB
    4 crate archives:            13.04 GB
    4 crate source checkouts:   268.46 MB
  Git db:                            0  B
    0 bare git repos:                0  B
    0 git repo checkouts:            0  B\n";

        assert_eq!(output_is, output_should);
    }

    #[allow(non_snake_case)]
    #[test]
    fn test_DirSizes_actually_empty() {
        // DirInfors to construct DirSizes from
        let bindir = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let git_repos_bare = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let git_checkouts = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let reg_cache = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let reg_src = DirInfo {
            dir_size: 0,
            file_number: 0,
        };
        let reg_index = DirInfo {
            dir_size: 0,
            file_number: 0,
        };

        let pb = PathBuf::from("/home/user/.cargo");

        // create a DirSizes object
        let dirSizes = DirSizes::new_manually(
            &bindir,
            &git_repos_bare,
            &git_checkouts,
            &reg_cache,
            &reg_src,
            &reg_index,
            &pb,
        );

        let output_is = format!("{}", &dirSizes);

        let output_should = "Cargo cache '/home/user/.cargo':

Total:                          0  B
  0 installed binaries:         0  B
  Registry:                     0  B
    Registry index:             0  B
    0 crate archives:           0  B
    0 crate source checkouts:   0  B
  Git db:                       0  B
    0 bare git repos:           0  B
    0 git repo checkouts:       0  B\n";

        assert_eq!(output_is, output_should);
    }
}

#[cfg(all(test, feature = "bench"))]
mod benchmarks {
    use super::*;
    use crate::test::black_box;
    use crate::test::Bencher;
    use std::path::PathBuf;

    #[bench]
    fn bench_pretty_print(b: &mut Bencher) {
        // DirInfors to construct DirSizes from
        let bindir = DirInfo {
            dir_size: 121_212,
            file_number: 31,
        };
        let git_repos_bare = DirInfo {
            dir_size: 121_212,
            file_number: 37,
        };
        let git_checkouts = DirInfo {
            dir_size: 34984,
            file_number: 8,
        };
        let reg_cache = DirInfo {
            dir_size: 89,
            file_number: 23445,
        };
        let reg_src = DirInfo {
            dir_size: 1_938_493_989,
            file_number: 123_909_849,
        };
        let reg_index = DirInfo {
            dir_size: 23,
            file_number: 12345,
        };

        let pb = PathBuf::from("/home/user/.cargo");
        // create a DirSizes object
        let dir_sizes = DirSizes::new_manually(
            &bindir,
            &git_repos_bare,
            &git_checkouts,
            &reg_cache,
            &reg_src,
            &reg_index,
            &pb,
        );

        b.iter(|| {
            let x = format!("{}", dir_sizes);
            let _ = black_box(x);
        });
    }
}
