# Files read out of a sandbox's checkpoint

**What this checks:** `GET /sandboxes/{id}/checkpoints/{name}/files` and
`.../file`. Before this a sandbox's checkpoint was memory pages, and nothing
read a file out of it without restoring the sandbox to it. boat lists and
downloads a snapshot's files; see the [comparison](../../../BOAT_PARITY.md)
and [how it works](../../../CHECKPOINTS.md#reading-one-from-outside).

The times below are the check's own stopwatch around one request. Each
includes restoring the checkpoint into a guest of its own. They are not a
benchmark.

## Method

`tools/check-checkpoint-files-kvm.py` runs an owned `hv2-sandboxd` with a
template of one vCPU and 512 MiB. The sandbox writes the following into
`/root/notes`, then starts a program of its own appending to `/root/ticker`
every second:

- `a.txt`, mode 600;
- a file named `a b` newline `c`;
- a symlink;
- a 1 MiB random file owned by 1000:1000, whose SHA-256 the sandbox records;
- a subdirectory.

Then it is checkpointed as `one`. After that the sandbox overwrites
`a.txt`, deletes the random file and adds another.

1. **Listing:** `/root/notes` in `one` lists exactly the five entries as
   taken. Each has the right type, mode, owner and size, the odd name
   included; a directory has no size.
2. **Reading:** `a.txt` reads `hello`, not what the sandbox wrote since.
   The 1 MiB file has the SHA-256 the sandbox recorded. The odd name reads.
3. **Untouched:** the sandbox still has its changed `a.txt` and lacks the
   deleted file. Its program is still appending, and it is listed once.
4. **Paused:** with the sandbox paused, `a.txt` reads from `one`. A resume
   then answers 201, which it does only for a paused sandbox, and the
   sandbox has its own `a.txt`.
5. **At once:** three reads of `a.txt` together all answer `hello`, two
   readers at a time.
6. **Refusals:**
   - no such checkpoint, no such sandbox, or no such path (404);
   - a file listed, a directory read, a symlink read, or a relative path
     (400);
   - a 65 MiB file in a second checkpoint (413).

## Result

All six cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, on a 24-thread host. From [`report.json`](report.json):

| | Took |
|---|---|
| Listing `/root/notes` | 0.061 s |
| Reading `a.txt` | 0.084 s |
| Reading the 1 MiB file | 0.132 s |
| Reading from a paused sandbox's checkpoint | 0.087 s |
| Three reads at once | 0.122 s |

## What this does not show

- **That the checkpoint's own programs were stopped.** Two reads of the
  ticker, 2.5 s apart, both had the 2 lines it had when taken. But it
  appends once a second, and each read is a new restore, so the reads
  would likely agree without the stop too.
- **A node with `--network`.** The reader's guest is given the template's
  network device with nothing behind it; that was not run here.
- **A sandbox with volumes**, whose mounts the reader does not have.
- **A cluster**, where the request is forwarded to the sandbox's node.
- **A larger guest.** 512 MiB; a reader is a guest the sandbox's size.
