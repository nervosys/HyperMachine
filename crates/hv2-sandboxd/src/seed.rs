//! A cloud-init seed: the small disk a cloud image reads its first-boot
//! instructions from.
//!
//! A distribution's cloud image has no account and no name until cloud-init
//! gives it them, and cloud-init looks for them in a "datasource". The one
//! that needs nothing but a disk is NoCloud: a filesystem labelled `cidata`
//! holding `user-data` and `meta-data`. This builds that filesystem, as an
//! image file a machine is given as a second, read-only disk.
//!
//! # Why a FAT image written here
//!
//! NoCloud accepts ISO 9660 or FAT. Both are usually made with a tool
//! (`genisoimage`, `mkfs.vfat` and `mcopy`), and a node that needed one
//! installed to create a machine would fail in a new way on the first node
//! without it. The filesystem needed is tiny and fixed: a handful of files in
//! the root directory of a FAT12 volume. That is written directly.
//!
//! The file names are not 8.3 names (`user-data` has nine characters before
//! any dot), so each has a long-name entry in front of its short one, which
//! is the part of FAT this has to get right for a guest to find them.
//!
//! # Two sizes
//!
//! A seed of text fits in 1 MiB with one sector to a cluster. One that also
//! carries the guest agent's binary does not, and FAT12 has room for only
//! 4,084 clusters, so that one is 15 MiB with eight sectors to a cluster.
//! The smaller is used whenever it is enough.
//!
//! # The guest agent
//!
//! A machine asked to run this project's agent gets two more files: the
//! agent's binary, and `vendor-data`, which NoCloud reads beside
//! `user-data`. The vendor data is a script, not a `#cloud-config`: a
//! caller's own user data replaces a vendor's configuration key for key, and
//! a caller who wrote `runcmd` would silently switch the agent off. A script
//! runs whatever the user data says.
//!
//! # What it does not do
//!
//! Subdirectories, files past the volume's size, or anything but writing an
//! image once. It is not a FAT implementation.

use std::path::Path;

const SECTOR: usize = 512;
const RESERVED: usize = 1;
const FATS: usize = 2;
const ROOT_ENTRIES: usize = 64;
const ROOT_SECTORS: usize = ROOT_ENTRIES * 32 / SECTOR;

/// How a volume is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    /// The whole volume, in sectors.
    sectors: usize,
    sectors_per_cluster: usize,
    /// Sectors in each copy of the table: enough 12-bit entries for it.
    fat_sectors: usize,
}

impl Geometry {
    /// 1 MiB, a sector to a cluster: text.
    const SMALL: Self = Self {
        sectors: 2048,
        sectors_per_cluster: 1,
        fat_sectors: 6,
    };
    /// 15 MiB, eight sectors to a cluster: text and a binary.
    const LARGE: Self = Self {
        sectors: 30_720,
        sectors_per_cluster: 8,
        fat_sectors: 12,
    };

    const fn data_start(self) -> usize {
        RESERVED + FATS * self.fat_sectors + ROOT_SECTORS
    }

    const fn clusters(self) -> usize {
        (self.sectors - self.data_start()) / self.sectors_per_cluster
    }

    const fn cluster_bytes(self) -> usize {
        self.sectors_per_cluster * SECTOR
    }

    /// The most the files together may hold, each rounded up to a cluster.
    const fn capacity(self) -> usize {
        self.clusters() * self.cluster_bytes()
    }

    /// How much of the volume `files` would take.
    fn needed(self, files: &[SeedFile<'_>]) -> usize {
        files
            .iter()
            .map(|f| f.contents.len().div_ceil(self.cluster_bytes()) * self.cluster_bytes())
            .sum()
    }
}

/// The label cloud-init's NoCloud datasource looks for.
pub(crate) const LABEL: &[u8; 11] = b"CIDATA     ";

/// The most user data a machine may be given: what the larger volume holds,
/// less room for the agent and the files the node writes itself.
pub(crate) const MAX_USER_DATA: usize = 1 << 20;

/// The most the agent's binary may be, for the same reason.
pub(crate) const MAX_AGENT: usize = 12 << 20;

/// The name the agent's binary has on the seed.
const AGENT_FILE: &str = "hv2-guest-agentd";

/// What the guest runs, once, to install the agent from the seed and start
/// it now and at every boot after. Plain `sh`, and only tools a distribution
/// with systemd has.
const AGENT_INSTALL: &str = r#"#!/bin/sh
# Written by the node this machine runs on: install its guest agent from the
# seed disk and start it. cloud-init runs this once for the instance.
set -e
seed=/run/hv2-seed
mkdir -p "$seed"
mount -o ro -L CIDATA "$seed"
install -m 0755 "$seed/hv2-guest-agentd" /usr/local/sbin/hv2-guest-agentd
umount "$seed"
cat > /etc/systemd/system/hv2-guest-agent.service <<'UNIT'
[Unit]
Description=HyperMachine guest agent

[Service]
ExecStart=/usr/local/sbin/hv2-guest-agentd
Restart=always
RestartSec=1

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now hv2-guest-agent.service
"#;

/// One file in the seed's root directory.
pub(crate) struct SeedFile<'a> {
    /// Its name as the guest sees it: ASCII, at most 26 characters.
    pub(crate) name: &'a str,
    pub(crate) contents: &'a [u8],
}

/// What a machine's seed is made from.
pub(crate) struct Seed<'a> {
    /// What tells cloud-init whether this is a first boot: it runs its
    /// once-per-instance steps again only when the ID changes, so a machine
    /// keeps one for its life.
    pub(crate) instance_id: &'a str,
    pub(crate) hostname: &'a str,
    /// The caller's cloud-init user data, if any.
    pub(crate) user_data: Option<&'a str>,
    /// The guest agent's binary, when the machine is to run it.
    pub(crate) agent: Option<&'a [u8]>,
}

impl Seed<'_> {
    /// The seed's image.
    pub(crate) fn image(&self) -> Result<Vec<u8>, String> {
        let meta = format!(
            "instance-id: {}\nlocal-hostname: {}\n",
            self.instance_id, self.hostname
        );
        // NoCloud wants the file there even when there is nothing to say.
        let user_data = self.user_data.unwrap_or("#cloud-config\n{}\n");
        let mut files = vec![
            SeedFile {
                name: "meta-data",
                contents: meta.as_bytes(),
            },
            SeedFile {
                name: "user-data",
                contents: user_data.as_bytes(),
            },
        ];
        if let Some(agent) = self.agent {
            files.push(SeedFile {
                name: "vendor-data",
                contents: AGENT_INSTALL.as_bytes(),
            });
            files.push(SeedFile {
                name: AGENT_FILE,
                contents: agent,
            });
        }
        image(&files)
    }

    /// Write the seed's image to `path`.
    pub(crate) fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.image()?).map_err(|e| format!("writing the cloud-init seed: {e}"))
    }
}

/// A FAT12 volume labelled [`LABEL`] holding `files` in its root directory:
/// the smaller layout when they fit it, the larger when they need it.
pub(crate) fn image(files: &[SeedFile<'_>]) -> Result<Vec<u8>, String> {
    let geometry = [Geometry::SMALL, Geometry::LARGE]
        .into_iter()
        .find(|g| g.needed(files) <= g.capacity())
        .ok_or_else(|| {
            format!(
                "the cloud-init seed holds at most {} KiB, and this is {} KiB",
                Geometry::LARGE.capacity() / 1024,
                Geometry::LARGE.needed(files).div_ceil(1024)
            )
        })?;
    let cluster_bytes = geometry.cluster_bytes();
    let mut disk = vec![0u8; geometry.sectors * SECTOR];

    // The boot sector: no code, only the description of the volume.
    let boot = &mut disk[..SECTOR];
    boot[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    boot[3..11].copy_from_slice(b"HYPERM  ");
    boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    boot[13] = geometry.sectors_per_cluster as u8;
    boot[14..16].copy_from_slice(&(RESERVED as u16).to_le_bytes());
    boot[16] = FATS as u8;
    boot[17..19].copy_from_slice(&(ROOT_ENTRIES as u16).to_le_bytes());
    boot[19..21].copy_from_slice(&(geometry.sectors as u16).to_le_bytes());
    boot[21] = 0xF8; // fixed disk
    boot[22..24].copy_from_slice(&(geometry.fat_sectors as u16).to_le_bytes());
    boot[24..26].copy_from_slice(&32u16.to_le_bytes()); // sectors per track
    boot[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    boot[36] = 0x80; // drive number
    boot[38] = 0x29; // the three fields after this are present
    boot[39..43].copy_from_slice(&0x4859_5045u32.to_le_bytes()); // volume ID
    boot[43..54].copy_from_slice(LABEL);
    boot[54..62].copy_from_slice(b"FAT12   ");
    boot[510] = 0x55;
    boot[511] = 0xAA;

    // The table: the media byte and an end mark in the two reserved entries,
    // then each file's clusters in a row, the last marked as the end.
    let mut table = vec![0u16; geometry.clusters() + 2];
    table[0] = 0xFF8;
    table[1] = 0xFFF;

    // The root directory: the label, then for each file its long-name
    // entries and its short one.
    let mut root = Vec::with_capacity(ROOT_ENTRIES * 32);
    let mut label = [0u8; 32];
    label[..11].copy_from_slice(LABEL);
    label[11] = 0x08; // volume label
    root.extend_from_slice(&label);

    let mut next_cluster = 2usize;
    for (index, file) in files.iter().enumerate() {
        let short = short_name(file.name, index)?;
        let clusters = file.contents.len().div_ceil(cluster_bytes);
        let first = if clusters == 0 { 0 } else { next_cluster };
        for i in 0..clusters {
            let cluster = next_cluster + i;
            table[cluster] = if i + 1 == clusters {
                0xFFF
            } else {
                (cluster + 1) as u16
            };
            let at =
                (geometry.data_start() + (cluster - 2) * geometry.sectors_per_cluster) * SECTOR;
            let chunk =
                &file.contents[i * cluster_bytes..file.contents.len().min((i + 1) * cluster_bytes)];
            disk[at..at + chunk.len()].copy_from_slice(chunk);
        }
        next_cluster += clusters;

        root.extend(long_name_entries(file.name, checksum(&short)));
        let mut entry = [0u8; 32];
        entry[..11].copy_from_slice(&short);
        entry[11] = 0x20; // archive: an ordinary file
        entry[26..28].copy_from_slice(&(first as u16).to_le_bytes());
        entry[28..32].copy_from_slice(&(file.contents.len() as u32).to_le_bytes());
        root.extend_from_slice(&entry);
    }
    if root.len() > ROOT_ENTRIES * 32 {
        return Err("the cloud-init seed's directory is full".to_string());
    }

    // Twelve bits an entry, two entries to three bytes.
    let mut packed = vec![0u8; geometry.fat_sectors * SECTOR];
    for (pair, entries) in table.chunks(2).enumerate() {
        let (a, b) = (entries[0], entries.get(1).copied().unwrap_or(0));
        packed[pair * 3] = a as u8;
        packed[pair * 3 + 1] = ((a >> 8) as u8 & 0x0F) | ((b as u8 & 0x0F) << 4);
        packed[pair * 3 + 2] = (b >> 4) as u8;
    }
    for copy in 0..FATS {
        let at = (RESERVED + copy * geometry.fat_sectors) * SECTOR;
        disk[at..at + packed.len()].copy_from_slice(&packed);
    }
    let at = (RESERVED + FATS * geometry.fat_sectors) * SECTOR;
    disk[at..at + root.len()].copy_from_slice(&root);
    Ok(disk)
}

/// The 8.3 name that stands behind a long one: the first six characters that
/// a short name may hold, upper-cased, then `~` and a number. The number is
/// the file's place in the directory, so no two are the same.
fn short_name(name: &str, index: usize) -> Result<[u8; 11], String> {
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    if name.is_empty() || name.len() > 26 || !plain || index >= 9 {
        return Err(format!(
            "{name:?} cannot be a file in the cloud-init seed: letters, digits, - _ . and at \
             most 26 characters, in at most nine files"
        ));
    }
    let mut short = [b' '; 11];
    let stem: Vec<u8> = name
        .bytes()
        .filter(|b| *b != b'.')
        .map(|b| b.to_ascii_uppercase())
        .take(6)
        .collect();
    short[..stem.len()].copy_from_slice(&stem);
    short[stem.len()] = b'~';
    short[stem.len() + 1] = b'1' + index as u8;
    Ok(short)
}

/// The checksum that ties a long-name entry to its short one.
fn checksum(short: &[u8; 11]) -> u8 {
    short.iter().fold(0u8, |sum, c| {
        ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(*c)
    })
}

/// The long-name entries for `name`, in the order they are stored: last
/// piece first. Each holds thirteen UTF-16 characters; the name is ended by
/// a zero and the rest filled with `0xFFFF`.
fn long_name_entries(name: &str, checksum: u8) -> Vec<u8> {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    if !units.len().is_multiple_of(13) {
        units.push(0);
        while !units.len().is_multiple_of(13) {
            units.push(0xFFFF);
        }
    }
    let pieces = units.len() / 13;
    let mut out = Vec::with_capacity(pieces * 32);
    for piece in (0..pieces).rev() {
        let chars = &units[piece * 13..(piece + 1) * 13];
        let mut entry = [0u8; 32];
        entry[0] = (piece + 1) as u8 | if piece + 1 == pieces { 0x40 } else { 0 };
        entry[11] = 0x0F; // long name
        entry[13] = checksum;
        // Five characters, then six, then two, around the fixed fields.
        for (i, at) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
            .into_iter()
            .enumerate()
        {
            entry[at..at + 2].copy_from_slice(&chars[i].to_le_bytes());
        }
        out.extend_from_slice(&entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the image back the way a guest would, from what its boot sector
    /// says: the table, the directory with its long names, and each file's
    /// chain of clusters.
    fn read_back(disk: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16_at = |at: usize| u16::from_le_bytes([disk[at], disk[at + 1]]) as usize;
        assert_eq!(u16_at(11), SECTOR);
        assert_eq!(u16_at(19) * SECTOR, disk.len(), "the size the volume says");
        let cluster_bytes = disk[13] as usize * SECTOR;
        let (reserved, fats, fat_sectors) = (u16_at(14), disk[16] as usize, u16_at(22));
        let root_entries = u16_at(17);
        let fat = &disk[reserved * SECTOR..(reserved + fat_sectors) * SECTOR];
        let entry = |n: usize| {
            let at = n / 2 * 3;
            if n.is_multiple_of(2) {
                fat[at] as usize | ((fat[at + 1] as usize & 0x0F) << 8)
            } else {
                (fat[at + 1] as usize >> 4) | ((fat[at + 2] as usize) << 4)
            }
        };
        let root_at = (reserved + fats * fat_sectors) * SECTOR;
        let data_at = root_at + root_entries * 32;
        // FAT12 is what the boot sector claims only below this many clusters.
        assert!((disk.len() - data_at) / cluster_bytes < 4085);
        let mut files = Vec::new();
        let mut long: Vec<u16> = Vec::new();
        for i in 0..root_entries {
            let e = &disk[root_at + i * 32..root_at + (i + 1) * 32];
            if e[0] == 0 {
                break;
            }
            if e[11] == 0x0F {
                let mut piece = Vec::new();
                for at in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30] {
                    piece.push(u16::from_le_bytes([e[at], e[at + 1]]));
                }
                // Stored last piece first.
                piece.append(&mut long);
                long = piece;
                continue;
            }
            if e[11] == 0x08 {
                continue;
            }
            let short: [u8; 11] = e[..11].try_into().unwrap();
            let name: Vec<u16> = long
                .drain(..)
                .take_while(|c| *c != 0 && *c != 0xFFFF)
                .collect();
            let name = String::from_utf16(&name).unwrap();
            let size = u32::from_le_bytes(e[28..32].try_into().unwrap()) as usize;
            let mut cluster = u16::from_le_bytes([e[26], e[27]]) as usize;
            let mut contents = Vec::new();
            while (2..0xFF8).contains(&cluster) {
                let at = data_at + (cluster - 2) * cluster_bytes;
                contents.extend_from_slice(&disk[at..at + cluster_bytes]);
                cluster = entry(cluster);
            }
            contents.truncate(size);
            assert!(!name.is_empty(), "{short:?} has no long name");
            files.push((name, contents));
        }
        files
    }

    #[test]
    fn a_seed_reads_back_as_the_files_put_in_it() {
        // One shorter than a sector, one an exact sector, one several and a
        // bit, one with a name longer than a long-name entry holds.
        let exact = vec![b'e'; SECTOR];
        let long = vec![b'l'; SECTOR * 3 + 17];
        let disk = image(&[
            SeedFile {
                name: "meta-data",
                contents: b"instance-id: i-1\n",
            },
            SeedFile {
                name: "user-data",
                contents: &exact,
            },
            SeedFile {
                name: "network-config",
                contents: &long,
            },
            SeedFile {
                name: "empty",
                contents: b"",
            },
        ])
        .unwrap();
        assert_eq!(disk.len(), 1 << 20);
        assert_eq!(&disk[510..512], &[0x55, 0xAA]);
        assert_eq!(&disk[43..54], LABEL);
        assert_eq!(&disk[54..62], b"FAT12   ");
        // Both copies of the table are the same.
        let fat = RESERVED * SECTOR;
        let fat_bytes = Geometry::SMALL.fat_sectors * SECTOR;
        assert_eq!(
            disk[fat..fat + fat_bytes],
            disk[fat + fat_bytes..fat + 2 * fat_bytes]
        );
        assert_eq!(&disk[fat..fat + 3], &[0xF8, 0xFF, 0xFF]);
        // The label is the directory's first entry.
        let root = (RESERVED + FATS * Geometry::SMALL.fat_sectors) * SECTOR;
        assert_eq!(
            (&disk[root..root + 11], disk[root + 11]),
            (&LABEL[..], 0x08)
        );

        let files = read_back(&disk);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["meta-data", "user-data", "network-config", "empty"]);
        assert_eq!(files[0].1, b"instance-id: i-1\n");
        assert_eq!(files[1].1, exact);
        assert_eq!(files[2].1, long);
        assert!(files[3].1.is_empty());
    }

    #[test]
    fn a_long_name_is_tied_to_its_short_one() {
        let short = short_name("user-data", 1).unwrap();
        assert_eq!(&short, b"USER-D~2   ");
        // A different short name has a different checksum, so a long name
        // cannot be read as belonging to the wrong file.
        assert_ne!(
            checksum(&short),
            checksum(&short_name("user-data", 0).unwrap())
        );
        let entries = long_name_entries("user-data", checksum(&short));
        assert_eq!(entries.len(), 32);
        assert_eq!(entries[0], 0x41, "the only piece, marked as the last");
        assert_eq!((entries[11], entries[13]), (0x0F, checksum(&short)));
        assert_eq!(&entries[1..3], &[b'u', 0]);
        // Fourteen characters take two entries, the second piece stored first.
        let two = long_name_entries("network-config", 0);
        assert_eq!(two.len(), 64);
        assert_eq!((two[0], two[32]), (0x42, 0x01));
        assert_eq!(&two[1..3], &[b'g', 0], "the fourteenth character");
        assert_eq!(&two[3..5], &[0, 0], "then the end of the name");
        assert_eq!(&two[5..7], &[0xFF, 0xFF], "then fill");
    }

    /// Text fits the small volume; a binary of a few megabytes moves the
    /// whole seed to the large one, eight sectors to a cluster, and both
    /// read back whole.
    #[test]
    fn the_volume_is_the_smaller_one_until_that_is_not_enough() {
        let fits = vec![1u8; Geometry::SMALL.capacity()];
        let small = image(&[SeedFile {
            name: "user-data",
            contents: &fits,
        }])
        .unwrap();
        assert_eq!((small.len(), small[13]), (1 << 20, 1));
        assert_eq!(read_back(&small)[0].1, fits);

        // One byte more than the small one holds.
        let over = vec![2u8; Geometry::SMALL.capacity() + 1];
        let large = image(&[SeedFile {
            name: "user-data",
            contents: &over,
        }])
        .unwrap();
        assert_eq!((large.len(), large[13]), (15 << 20, 8));
        assert_eq!(read_back(&large)[0].1, over);

        // A binary whose length is not a whole cluster, with text beside it:
        // each file starts on its own cluster and none runs into the next.
        let binary: Vec<u8> = (0..3_000_017u32).map(|i| (i % 251) as u8).collect();
        let disk = image(&[
            SeedFile {
                name: "meta-data",
                contents: b"instance-id: i-2\n",
            },
            SeedFile {
                name: "hv2-guest-agentd",
                contents: &binary,
            },
            SeedFile {
                name: "vendor-data",
                contents: b"#!/bin/sh\n",
            },
        ])
        .unwrap();
        let files = read_back(&disk);
        assert_eq!(files[0].1, b"instance-id: i-2\n");
        assert_eq!(files[1].0, "hv2-guest-agentd");
        assert_eq!(files[1].1, binary);
        assert_eq!(files[2].1, b"#!/bin/sh\n");
    }

    #[test]
    fn what_cannot_be_a_seed_is_refused() {
        let too_much = vec![0u8; Geometry::LARGE.capacity() + 1];
        let refused = image(&[SeedFile {
            name: "user-data",
            contents: &too_much,
        }])
        .unwrap_err();
        assert!(refused.contains("at most"), "{refused}");
        for name in [
            "",
            "a b",
            "sub/dir",
            "a-name-that-is-longer-than-this-allows",
        ] {
            assert!(
                image(&[SeedFile {
                    name,
                    contents: b"x"
                }])
                .is_err(),
                "{name:?}"
            );
        }
        // The most the large one holds, exactly, is not refused.
        let full = vec![1u8; Geometry::LARGE.capacity()];
        let disk = image(&[SeedFile {
            name: "user-data",
            contents: &full,
        }])
        .unwrap();
        assert_eq!(read_back(&disk)[0].1, full);
        // What the node lets a caller and an agent take leaves room for the
        // files it writes itself.
        assert!(MAX_USER_DATA + MAX_AGENT + (64 << 10) <= Geometry::LARGE.capacity());
    }

    #[test]
    fn a_machine_s_seed_names_it_and_carries_its_user_data() {
        let disk = Seed {
            instance_id: "vm-web-01",
            hostname: "web-01",
            user_data: Some("#cloud-config\npassword: x\n"),
            agent: None,
        }
        .image()
        .unwrap();
        assert_eq!(disk.len(), 1 << 20);
        let files = read_back(&disk);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "meta-data");
        assert_eq!(
            files[0].1,
            b"instance-id: vm-web-01\nlocal-hostname: web-01\n"
        );
        assert_eq!(files[1].0, "user-data");
        assert_eq!(files[1].1, b"#cloud-config\npassword: x\n");
    }

    /// With an agent the seed also carries its binary and the script that
    /// installs it, and a machine given no user data still has a `user-data`
    /// for cloud-init to find.
    #[test]
    fn a_seed_with_an_agent_carries_its_binary_and_how_to_install_it() {
        let binary = vec![0x7Fu8; 2_500_000];
        let disk = Seed {
            instance_id: "vm-a",
            hostname: "a",
            user_data: None,
            agent: Some(&binary),
        }
        .image()
        .unwrap();
        let files = read_back(&disk);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["meta-data", "user-data", "vendor-data", "hv2-guest-agentd"]
        );
        assert_eq!(files[1].1, b"#cloud-config\n{}\n");
        let script = String::from_utf8(files[2].1.clone()).unwrap();
        // A script, so that a caller's own cloud-config cannot replace it.
        assert!(script.starts_with("#!/bin/sh\n"), "{script}");
        for needed in [
            "mount -o ro -L CIDATA",
            "/usr/local/sbin/hv2-guest-agentd",
            "systemctl enable --now hv2-guest-agent.service",
        ] {
            assert!(script.contains(needed), "{needed}");
        }
        assert_eq!(files[3].1, binary);
    }
}
