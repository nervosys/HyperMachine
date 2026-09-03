window.BENCHMARK_DATA = {
  "lastUpdate": 1788465278263,
  "repoUrl": "https://github.com/nervosys/HyperMachine",
  "entries": {
    "HyperMachine Benchmarks": [
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f129e3136f78029991487d7a1e69d2fe69531e80",
          "message": "fix(ci): the Benchmarks job has never run to completion (#74)\n\n* fix(ci): the Benchmarks job has never run to completion\n\n#71 fixed the missing protoc, which was real, and revealed the next\nfailure rather than the last one:\n\n  fatal: couldn't find remote ref gh-pages\n  Error: The process 'git.exe' failed with exit code 128\n\ngithub-action-benchmark is configured with auto-push: true, which stores\nhistory on a gh-pages branch. This repository has none -- git ls-remote\n--heads origin gh-pages returns nothing -- so the action fails in its\nfirst git step, before a single result is recorded.\n\nTurning off auto-push and skipping the gh-pages fetch lets the benchmarks\nrun and report, which is the part that has been missing since the job was\nwritten.\n\nWhat this deliberately does not do is create the gh-pages branch. That is\nthe other valid fix, and arguably the intended design: the job also sets\nalert-threshold, comment-on-alert and alert-comment-cc-users, none of\nwhich mean anything without stored history. But creating that branch on a\npublic repository has GitHub Pages implications, and that should be a\ndecision rather than a side effect of turning a red job green. The\ncomment says which flags to flip once it exists.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n* fix(ci): create the branch the Benchmarks job stores its history on\n\nCorrecting my own first attempt at this. I turned auto-push off to avoid\ncreating a gh-pages branch, on the grounds that it might have GitHub\nPages implications for a public repository. Two things were wrong with\nthat.\n\nIt did not work. skip-fetch-gh-pages skips the fetch, not the switch, so\nthe action still ran `git switch gh-pages` and failed with \"invalid\nreference: gh-pages\" -- save-data-file defaults to true and needs the\nbranch to write to. Disabling the fetch addressed the symptom I had seen\nrather than what the action actually does.\n\nAnd the caution was unfounded: GitHub Pages is not enabled for this\nrepository (the API returns 404 for it), so a branch of that name\npublishes nothing. Enabling Pages would remain a deliberate, separate\nact.\n\nSo the branch now exists, as an orphan carrying only dev/bench data and\nno source, and auto-push goes back to true -- which is what the job was\nalways configured for. alert-threshold, comment-on-alert and\nalert-comment-cc-users only mean something with stored history, and\nturning that off would have left three settings describing behaviour that\ncould not happen.\n\nThe comment records one more thing worth knowing: that branch must not be\nprotected. The action commits to it directly, and a pull-request rule\nbreaks it -- exactly the mistake the CLA workflow made by pointing at\nmaster.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n---------\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-01T15:16:59-07:00",
          "tree_id": "e6149317f5c29ee9e129ec647029e86801990a3f",
          "url": "https://github.com/nervosys/HyperMachine/commit/f129e3136f78029991487d7a1e69d2fe69531e80"
        },
        "date": 1788302214269,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 516.949,
            "range": "+/- 1.672",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3391.523,
            "range": "+/- 12.916",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 380.51,
            "range": "+/- 1.175",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1008.092,
            "range": "+/- 4.949",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 354.284,
            "range": "+/- 0.855",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12619.59,
            "range": "+/- 66.775",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 583.039,
            "range": "+/- 1.533",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 7169.296,
            "range": "+/- 22.308",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 446.751,
            "range": "+/- 1.889",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1108.482,
            "range": "+/- 5.59",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 414.343,
            "range": "+/- 2.625",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12712.576,
            "range": "+/- 44.828",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2515.085,
            "range": "+/- 9.014",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 107.408,
            "range": "+/- 2.035",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.728,
            "range": "+/- 0.645",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1167.461,
            "range": "+/- 8.324",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2017.14,
            "range": "+/- 56.956",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 623.548,
            "range": "+/- 5.274",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 795.607,
            "range": "+/- 6.545",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 936.649,
            "range": "+/- 1.675",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10718.027,
            "range": "+/- 12.249",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 440.065,
            "range": "+/- 0.951",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2873.619,
            "range": "+/- 1.993",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 319.285,
            "range": "+/- 0.937",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74094.245,
            "range": "+/- 755.882",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12657.504,
            "range": "+/- 136.223",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 356.332,
            "range": "+/- 1.043",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.533,
            "range": "+/- 0.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.323,
            "range": "+/- 0.243",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.699,
            "range": "+/- 0.105",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1422.621,
            "range": "+/- 6.936",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.247,
            "range": "+/- 0.123",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2141.763,
            "range": "+/- 16.02",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16409.829,
            "range": "+/- 65.894",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 735.321,
            "range": "+/- 0.798",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663784.168,
            "range": "+/- 1095.885",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10448.188,
            "range": "+/- 13.686",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 251.03,
            "range": "+/- 0.729",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2685.446,
            "range": "+/- 1.519",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 129.126,
            "range": "+/- 0.941",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41532.86,
            "range": "+/- 60.135",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2247.206,
            "range": "+/- 6.913",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2037068.96,
            "range": "+/- 12139.179",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31285.301,
            "range": "+/- 119.927",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 802.592,
            "range": "+/- 4.258",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8065.123,
            "range": "+/- 29.238",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 298.696,
            "range": "+/- 1.778",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125559.94,
            "range": "+/- 727.557",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7356.742,
            "range": "+/- 68.487",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8260.416,
            "range": "+/- 81.528",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d522bf63d936ddb67652f0c8114a961ad7c428f7",
          "message": "fix(ci): the CLA check stored signatures on a branch it cannot write to (#75)\n\nWith the action resolving for the first time (#71 pinned it to a tag that\nexists), it got far enough to reveal the next problem:\n\n  Error occurred when creating the signed contributors file: Repository\n  rule violations found. Changes must be made through a pull request.\n  Make sure the branch where signatures are stored is NOT protected.\n\nThe workflow set branch: \"master\", and master's ruleset requires a pull\nrequest for any change. The action records a signature by committing\nsignatures/cla.json to that branch, so the commit was refused. No amount\nof signing could have turned this check green -- the contributor comment\nwould have been accepted and then failed to record.\n\nPoints it at cla-signatures instead, a branch created for this and left\nunprotected, as the action's own error message asks for.\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-01T16:18:06-07:00",
          "tree_id": "248f97c0f502049cdcf3e846bbe1a070b25662f3",
          "url": "https://github.com/nervosys/HyperMachine/commit/d522bf63d936ddb67652f0c8114a961ad7c428f7"
        },
        "date": 1788305556155,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 543.144,
            "range": "+/- 2.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3393.619,
            "range": "+/- 8.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 469.828,
            "range": "+/- 5.946",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1019.256,
            "range": "+/- 3.233",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 419.79,
            "range": "+/- 10.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12430.69,
            "range": "+/- 49.682",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 609.306,
            "range": "+/- 0.996",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3738.524,
            "range": "+/- 12.277",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 475.159,
            "range": "+/- 1.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1173.708,
            "range": "+/- 7.105",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 432.458,
            "range": "+/- 1.567",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13208.65,
            "range": "+/- 41.477",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2664.626,
            "range": "+/- 9.187",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.563,
            "range": "+/- 0.396",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.258,
            "range": "+/- 0.841",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1244.224,
            "range": "+/- 5.354",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2002.907,
            "range": "+/- 9.702",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 655.879,
            "range": "+/- 2.595",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 876.3,
            "range": "+/- 4.56",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1053.255,
            "range": "+/- 1.284",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12003.115,
            "range": "+/- 5.898",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.592,
            "range": "+/- 0.708",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3245.542,
            "range": "+/- 2.802",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 367.55,
            "range": "+/- 4.294",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75580.92,
            "range": "+/- 126.088",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11416.087,
            "range": "+/- 88.684",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 394.58,
            "range": "+/- 0.635",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.351,
            "range": "+/- 0.293",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.711,
            "range": "+/- 0.236",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.024,
            "range": "+/- 0.089",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1574.917,
            "range": "+/- 4.514",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.083,
            "range": "+/- 0.618",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2084.47,
            "range": "+/- 6.842",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16227.831,
            "range": "+/- 198.192",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 846.987,
            "range": "+/- 1.366",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747075.342,
            "range": "+/- 327.893",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11811.008,
            "range": "+/- 6.316",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 283.629,
            "range": "+/- 0.295",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3041.56,
            "range": "+/- 3.884",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.872,
            "range": "+/- 0.386",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46734.87,
            "range": "+/- 27.919",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2559.429,
            "range": "+/- 18.016",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2251746.435,
            "range": "+/- 9324.857",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35101.279,
            "range": "+/- 104.818",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 899.457,
            "range": "+/- 3.992",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9126.163,
            "range": "+/- 56.06",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 332.791,
            "range": "+/- 2.431",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140128.364,
            "range": "+/- 493.72",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6885.94,
            "range": "+/- 69.242",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7532.698,
            "range": "+/- 47.716",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c1c58dd62b145687e6442fe6b1eb03b22ff0bdf3",
          "message": "fix(ci): the CLA gate refused to run for the comment that signs it (#82)\n\nThe job's condition admitted two events: any `pull_request_target`, and an\n`issue_comment` whose body is exactly `recheck`. The signing phrase was\nnot in the list, so the one comment that records a signature was the one\ncomment the job would not run for.\n\nThe failure is silent from the pull request's side. The check simply stays\nred, while the run appears in the Actions list as `issue_comment /\nskipped` and `signatures/cla.json` stays `{\"signedContributors\": []}`.\nSigning again does not help, because the second attempt is skipped for the\nsame reason as the first.\n\nThe action's own README guards on both strings. This restores the second.\n\nThat makes three separate faults in this workflow, each of which alone was\nenough to make signing impossible: an action reference that did not\nresolve (fixed in #75), a signatures branch the token could not write\n(fixed in #75), and this. The check has never once recorded a signature.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T10:26:32-07:00",
          "tree_id": "89363f48cdb344db4e7c2b962cbfd7935c27c974",
          "url": "https://github.com/nervosys/HyperMachine/commit/c1c58dd62b145687e6442fe6b1eb03b22ff0bdf3"
        },
        "date": 1788372528696,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 527.792,
            "range": "+/- 0.859",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3523.342,
            "range": "+/- 16.938",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 404.373,
            "range": "+/- 0.888",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1013.337,
            "range": "+/- 3.225",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 373.043,
            "range": "+/- 1.064",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12494.556,
            "range": "+/- 61.658",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 635.684,
            "range": "+/- 1.5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3858.544,
            "range": "+/- 13.112",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 479.02,
            "range": "+/- 0.264",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1186.569,
            "range": "+/- 4.569",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 445.586,
            "range": "+/- 1.142",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13455.533,
            "range": "+/- 24.799",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2684.609,
            "range": "+/- 7.392",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.906,
            "range": "+/- 0.255",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 130.632,
            "range": "+/- 5.89",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1210.016,
            "range": "+/- 4.476",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1962.107,
            "range": "+/- 9.248",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.014,
            "range": "+/- 1.131",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 823.372,
            "range": "+/- 1.541",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1062.179,
            "range": "+/- 1.357",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12012.877,
            "range": "+/- 6.882",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.172,
            "range": "+/- 0.519",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3244.128,
            "range": "+/- 1.217",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 358.682,
            "range": "+/- 0.948",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 81614.872,
            "range": "+/- 816.925",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11519.594,
            "range": "+/- 93.059",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 407.031,
            "range": "+/- 0.944",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.7,
            "range": "+/- 0.169",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.086,
            "range": "+/- 0.417",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.834,
            "range": "+/- 0.224",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1601.152,
            "range": "+/- 2.521",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 38.164,
            "range": "+/- 0.369",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2004.402,
            "range": "+/- 7.922",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15424.976,
            "range": "+/- 55.074",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 838.49,
            "range": "+/- 0.579",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 746871.856,
            "range": "+/- 297.991",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11815.675,
            "range": "+/- 13.907",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 291.483,
            "range": "+/- 0.38",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3045.561,
            "range": "+/- 6.398",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 148.859,
            "range": "+/- 0.301",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46825.308,
            "range": "+/- 23.103",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2555.452,
            "range": "+/- 16.035",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2229793.043,
            "range": "+/- 5428.468",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35118.931,
            "range": "+/- 83.006",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 901.148,
            "range": "+/- 5.423",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9165.989,
            "range": "+/- 56.104",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 327.883,
            "range": "+/- 1.027",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 139211.401,
            "range": "+/- 224.213",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6831.951,
            "range": "+/- 50.253",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7575.794,
            "range": "+/- 33.54",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "375a9345cf7be4852b78efa9f1d374b522c6a419",
          "message": "fix(ci): the baseline comparison failed builds on measurement noise (#76)\n\n* fix(ci): the baseline comparison failed builds on measurement noise\n\nFixing the Benchmarks job in #74 made this one visible for the first\ntime: benchmark-comparison declares `needs: benchmark`, and that job had\nnever completed a run, so this had never executed either.\n\nIt greps criterion's output for the word \"regressed\" and exited 1 on a\nmatch, with no threshold. Criterion prints that line for any benchmark\nmeasured slower than its baseline, however slightly, and the two runs\nbeing compared happen on one shared CI runner, minutes apart, with a full\nrebuild in between. A one percent wobble failed the build exactly as a\ntwo hundred percent regression would.\n\nThe evidence that this is noise rather than signal: it failed\nidentically, with a dozen \"Performance has regressed\" lines, on three\nDependabot pull requests bumping a Terraform provider, a Docker base\nimage and tock-registers. None of those can affect hv2-core's crypto\nbenchmarks.\n\nAlso worth noting: every other step in that job already carries\ncontinue-on-error, so the job tolerated the benchmarks themselves failing\nand then failed on noise in their output.\n\nReal regression alerting already exists and has both a threshold and a\nbaseline drawn from stored history -- github-action-benchmark at\nalert-threshold 150%, in the job above. This step's value is putting the\nnumbers in front of a reviewer, which it still does; it just no longer\nfails a build for having measured something on a busy machine.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n* fix(ci): pull requests were writing the benchmark baseline they compare to\n\n`auto-push` was unconditional, so every event that ran this workflow\ncommitted its measurements to `gh-pages`. Of the first 20 stored\nmeasurements, 18 came from pull-request branches and only 2 from master\n-- and several of those branches have since been deleted, so the\nbaseline is largely measurements of work that was never merged.\n\nThat is a better explanation for the comparison failures than runner\nnoise alone: a master run was not comparing itself against the previous\nmaster run, it was comparing itself against whichever pull request\nhappened to benchmark last.\n\nHistory now comes from pushes only. A pull request still runs the\nbenchmarks and still receives its comparison comment; it just no longer\nrecords itself as the thing the next run measures against.\n\nThe stored history is left as it is. Pruning it means rewriting a data\nbranch, which is the repository owner's call, and the entries are\nharmless once nothing new is appended from a pull request.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\n* fix(ci): correct a claim I made about this repository publishing nothing\n\nThe comment added in #74 said GitHub Pages was not enabled here, so\nnothing committed to `gh-pages` is published. That is wrong, and I wrote\nit. Pages is enabled with `source.branch` set to `gh-pages`:\n\n  {\"status\":\"errored\",\"html_url\":\"https://nervosys.github.io/HyperMachine/\",\n   \"build_type\":\"legacy\",\"source\":{\"branch\":\"gh-pages\",\"path\":\"/\"},\n   \"public\":true}\n\nThe 404 that led me to the wrong conclusion comes from the Pages builds\nfailing, not from Pages being off. The two most recent builds both report\n\"Page build failed\".\n\nThis matters beyond the comment. Everything the benchmark action commits\nis served publicly at nervosys.github.io/HyperMachine/dev/bench/, so the\n18 pull-request measurements described in the previous commit are on a\npublic page rather than in a private data file.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\n---------\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T12:14:29-07:00",
          "tree_id": "4243a38c142200cdbe5d96bdcd31707d373ebd2a",
          "url": "https://github.com/nervosys/HyperMachine/commit/375a9345cf7be4852b78efa9f1d374b522c6a419"
        },
        "date": 1788377605668,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 531.149,
            "range": "+/- 1.447",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3360.658,
            "range": "+/- 14.661",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 508.358,
            "range": "+/- 2.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.89,
            "range": "+/- 4.82",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 369.028,
            "range": "+/- 0.43",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13585.393,
            "range": "+/- 52.685",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 614.701,
            "range": "+/- 1.31",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3761.219,
            "range": "+/- 9.237",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 479.526,
            "range": "+/- 1.172",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1183.922,
            "range": "+/- 3.651",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 447.639,
            "range": "+/- 1.334",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13413.219,
            "range": "+/- 27.816",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2669.012,
            "range": "+/- 6.199",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.177,
            "range": "+/- 0.583",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 121.14,
            "range": "+/- 1.711",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1191.741,
            "range": "+/- 1.594",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1906.118,
            "range": "+/- 2.263",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 663.826,
            "range": "+/- 1.873",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 843.863,
            "range": "+/- 2.785",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1054.433,
            "range": "+/- 1.007",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12088.013,
            "range": "+/- 37.274",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 495.842,
            "range": "+/- 0.688",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3238.734,
            "range": "+/- 1.271",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 357.683,
            "range": "+/- 0.688",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75661.468,
            "range": "+/- 397.951",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11326.868,
            "range": "+/- 114.244",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 402.721,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.695,
            "range": "+/- 0.057",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.422,
            "range": "+/- 0.275",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.984,
            "range": "+/- 0.037",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1611.912,
            "range": "+/- 6.301",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.54,
            "range": "+/- 0.102",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2164.033,
            "range": "+/- 31.445",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15897.524,
            "range": "+/- 156.215",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 847.56,
            "range": "+/- 2.812",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747126.844,
            "range": "+/- 264.616",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 12243.73,
            "range": "+/- 142.528",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 282.587,
            "range": "+/- 0.991",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3037.284,
            "range": "+/- 2.75",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 141.365,
            "range": "+/- 0.863",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46819.424,
            "range": "+/- 29.108",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2537.281,
            "range": "+/- 6.833",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2248741.87,
            "range": "+/- 8063.661",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35324.211,
            "range": "+/- 123.501",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 983.746,
            "range": "+/- 20.413",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9127.479,
            "range": "+/- 52.591",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.622,
            "range": "+/- 1.43",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140667.6,
            "range": "+/- 676.697",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7230.012,
            "range": "+/- 90.719",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7568.876,
            "range": "+/- 37.232",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "edff5253a3f5fdc8ddbeedec7c21cf6148dc7847",
          "message": "fix(agent): VM identifiers could collide, and one VM would replace another (#83)\n\n`uuid_v4` was neither a UUID nor unique:\n\n    let timestamp = SystemTime::now()...as_nanos();\n    format!(\"{:032x}\", timestamp)\n\nTwo calls landing in the same clock tick return the same string, and\n`LocalVmHost::create` finishes with `self.vms.write().insert(vm_id, ..)`.\nThe second VM therefore replaces the first, silently: no error, and\n`vm_count` reports one where two were created.\n\nmacOS has coarser `SystemTime` resolution than Windows or Linux, which is\nwhy CI caught this on the macOS lane only, in `list_reports_every_vm`:\n\n    assertion `left == right` failed\n      left: [\"b\"]\n     right: [\"a\", \"b\"]\n\nIdentifiers now come from the OS CSPRNG, so they are distinct and\nunpredictable. Ownership is still enforced by session and capability\nchecks rather than by an id being hard to guess -- this stops one agent's\nVM from taking another's place by accident, nothing more.\n\nThe function is renamed `fresh_id` because it never produced a UUID, and\nthe duplicate copy in communication.rs is removed rather than fixed\ntwice.\n\nOn the test: creating VMs in a loop and waiting for a duplicate only\nfails where the clock happens to be coarse, so it proves nothing on the\nplatforms where it passes -- exactly the trap the original test fell\ninto. The new test asserts the part that does not depend on timing. A\nnanosecond count since the epoch is about 2^61, and `{:032x}` pads it to\n32 digits, so every clock-derived id begins with sixteen zeros while\nrandom ones share no prefix. Verified by putting the old generator back:\nthe new test fails, and passes again once the fix returns.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T15:46:54-07:00",
          "tree_id": "ec5082f19884ef7a6226ea233cf7aa17b8c61536",
          "url": "https://github.com/nervosys/HyperMachine/commit/edff5253a3f5fdc8ddbeedec7c21cf6148dc7847"
        },
        "date": 1788390200331,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.216,
            "range": "+/- 0.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2639.825,
            "range": "+/- 6.161",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 391.34,
            "range": "+/- 0.419",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 780.15,
            "range": "+/- 2.384",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 284.109,
            "range": "+/- 0.519",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9532.343,
            "range": "+/- 36.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 485.422,
            "range": "+/- 1.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2996.478,
            "range": "+/- 13.686",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 366.076,
            "range": "+/- 0.797",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 909.977,
            "range": "+/- 2.52",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 336.408,
            "range": "+/- 0.954",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10407.399,
            "range": "+/- 30.521",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2054.461,
            "range": "+/- 5.025",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 77.882,
            "range": "+/- 0.327",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 94.929,
            "range": "+/- 0.362",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 922.932,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1494.711,
            "range": "+/- 1.228",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 509.901,
            "range": "+/- 2.138",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 644.143,
            "range": "+/- 0.748",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 816.526,
            "range": "+/- 1.194",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9286.236,
            "range": "+/- 3.809",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 383.922,
            "range": "+/- 0.542",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2509.212,
            "range": "+/- 0.74",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 276.589,
            "range": "+/- 0.385",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 61224.561,
            "range": "+/- 242.487",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8686.892,
            "range": "+/- 64.37",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 307.009,
            "range": "+/- 0.692",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.129,
            "range": "+/- 0.027",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 79.398,
            "range": "+/- 0.111",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.222,
            "range": "+/- 0.056",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1223.004,
            "range": "+/- 2.489",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 26.506,
            "range": "+/- 0.044",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1625.097,
            "range": "+/- 6.692",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12477.025,
            "range": "+/- 131.265",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 656.692,
            "range": "+/- 1.198",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579097.014,
            "range": "+/- 250.314",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9173.912,
            "range": "+/- 14.7",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 218.519,
            "range": "+/- 0.334",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2386.21,
            "range": "+/- 7.368",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 108.532,
            "range": "+/- 0.199",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36237.652,
            "range": "+/- 9.812",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2121.863,
            "range": "+/- 25.334",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1774060.778,
            "range": "+/- 18887.329",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27951.898,
            "range": "+/- 329.818",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 747.753,
            "range": "+/- 8.975",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8109.431,
            "range": "+/- 120.831",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 256.997,
            "range": "+/- 1.362",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 107771.416,
            "range": "+/- 144.066",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5305.616,
            "range": "+/- 28.849",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 6028.36,
            "range": "+/- 88.263",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "49699333+dependabot[bot]@users.noreply.github.com",
            "name": "dependabot[bot]",
            "username": "dependabot[bot]"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5f8e52260a78af7b2bde01f66f5b9f767b5baf4a",
          "message": "deps(deps): bump tock-registers from 0.9.0 to 0.10.1 (#48)\n\nBumps [tock-registers](https://github.com/tock/tock-registers) from 0.9.0 to 0.10.1.\n- [Changelog](https://github.com/tock/tock-registers/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/tock/tock-registers/compare/v0.9.0...v0.10.1)\n\n---\nupdated-dependencies:\n- dependency-name: tock-registers\n  dependency-version: 0.10.1\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>",
          "timestamp": "2026-09-02T17:38:30-07:00",
          "tree_id": "24ce8c49b2f75942a680aab0f2ada7f142c35d5a",
          "url": "https://github.com/nervosys/HyperMachine/commit/5f8e52260a78af7b2bde01f66f5b9f767b5baf4a"
        },
        "date": 1788397021091,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 641.812,
            "range": "+/- 12.544",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3357.215,
            "range": "+/- 16.614",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 455.418,
            "range": "+/- 4.349",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1106.874,
            "range": "+/- 13.517",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 413.08,
            "range": "+/- 4.136",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13195.292,
            "range": "+/- 99.359",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 640.736,
            "range": "+/- 5.294",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3896.177,
            "range": "+/- 19.43",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 529.059,
            "range": "+/- 6.069",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1179.323,
            "range": "+/- 3.842",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 476.612,
            "range": "+/- 5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 18488.337,
            "range": "+/- 152.44",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2762.99,
            "range": "+/- 6.132",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.676,
            "range": "+/- 0.213",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.431,
            "range": "+/- 0.555",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1181.221,
            "range": "+/- 3.519",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1970.561,
            "range": "+/- 30.315",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 645.344,
            "range": "+/- 1.254",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 834.958,
            "range": "+/- 4.192",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1068.227,
            "range": "+/- 2.766",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12079.213,
            "range": "+/- 16.198",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 496.804,
            "range": "+/- 0.572",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3262.019,
            "range": "+/- 4.035",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 360.184,
            "range": "+/- 0.662",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 82368.366,
            "range": "+/- 792.263",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11079.972,
            "range": "+/- 67.291",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 398.035,
            "range": "+/- 0.67",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.972,
            "range": "+/- 0.091",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.796,
            "range": "+/- 0.582",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.146,
            "range": "+/- 0.067",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1633.998,
            "range": "+/- 4.551",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.878,
            "range": "+/- 0.174",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1991.023,
            "range": "+/- 5.836",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15706.979,
            "range": "+/- 115.926",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 837.799,
            "range": "+/- 0.872",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 748592.662,
            "range": "+/- 707.709",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11764.322,
            "range": "+/- 6.107",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 278.177,
            "range": "+/- 0.861",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3028.298,
            "range": "+/- 1.073",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 137.615,
            "range": "+/- 0.254",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46845.578,
            "range": "+/- 64.074",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2557.2,
            "range": "+/- 15.903",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2244564.609,
            "range": "+/- 8408.364",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35260.971,
            "range": "+/- 81.224",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 896.673,
            "range": "+/- 3.545",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9129.704,
            "range": "+/- 38.348",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.522,
            "range": "+/- 1.457",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 141826.336,
            "range": "+/- 1428.589",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7388.787,
            "range": "+/- 77.889",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7684.801,
            "range": "+/- 39.945",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "56753f56aad40b1c9823661173f225ee7c9f3cc8",
          "message": "chore: remove wasmtime, and the WASM claims that had no code behind them (#80)\n\nDependabot opened #63 to bump wasmtime 24 -> 48. Nothing in the workspace\nuses wasmtime: `grep -rl wasmtime --include=*.rs crates/` is empty. It was\ndeclared, made optional behind a `wasm-scripts` feature on hv2-agent, and\nthat feature enables no code. Bumping a dependency nothing compiles buys\nnothing, so this removes it instead.\n\nThe removal also drops fxhash from the graph, which is why the\nacknowledged-warnings table in SECURITY_AUDIT.md is now two rows shorter.\n\nThe larger problem is what the docs said about it. Rhai scripting is real\n-- `hv2-agent/src/script.rs` builds a Rhai engine with an operation cap, a\nstring-size cap and expression-depth limits, gated on `Capability::VmRead`.\nWASM scripting was described alongside it as though it were equally real:\n\n  - The MITRE mapping claimed \"Custom - WASM | wasmtime capability-based |\n    Mitigated\", and showed a `Config::new()` / `consume_fuel` /\n    `epoch_interruption` block that exists nowhere in this repository. That\n    block is replaced with the Rhai limits that are actually applied.\n  - The audit's input-validation table listed \"WASM modules | wasmtime\n    sandbox | Memory limits enforced\". There are no WASM modules.\n  - The agent-skill description, the AgentSkill schema, the request schema\n    and AGENTIC_ONTOLOGY.md all offered `script_type: \"wasm\"` and\n    base64-encoded WASM. An API that accepts a value nothing implements is\n    worse than one that does not offer it, so the enum is `[\"rhai\"]`.\n\nTwo neighbouring facts in the same files were wrong for unrelated reasons\nand are corrected while here: bincode is a direct dependency of hv2-core\nfor snapshot serialisation, not something bootloader drags in, and `paste`\narrives through image/rav1e under eframe in hm-gui. Both were checked with\n`cargo tree -i`.\n\n`cargo check --workspace --all-targets` is clean and `cargo test -p\nhv2-agent -p hv2-api` passes.\n\nCloses #63.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T18:52:33-07:00",
          "tree_id": "ae27536caf81ec2384ed8c8f5324b42c547ab2a0",
          "url": "https://github.com/nervosys/HyperMachine/commit/56753f56aad40b1c9823661173f225ee7c9f3cc8"
        },
        "date": 1788401464017,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 335.433,
            "range": "+/- 0.786",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 1956.049,
            "range": "+/- 13.974",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 266.923,
            "range": "+/- 0.866",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 639.788,
            "range": "+/- 3.101",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 242.361,
            "range": "+/- 0.428",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 10536.79,
            "range": "+/- 122.557",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 378.006,
            "range": "+/- 1.409",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2227.223,
            "range": "+/- 13.914",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 298.676,
            "range": "+/- 0.963",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 710.953,
            "range": "+/- 2.676",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 287.504,
            "range": "+/- 2.005",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 9912.372,
            "range": "+/- 30.719",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 1756.79,
            "range": "+/- 4.754",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 65.075,
            "range": "+/- 0.537",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 76.075,
            "range": "+/- 0.285",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 774.13,
            "range": "+/- 3.064",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1228.369,
            "range": "+/- 2.899",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 414.513,
            "range": "+/- 1.097",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 525.54,
            "range": "+/- 1.232",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 694.415,
            "range": "+/- 1.475",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 7887.649,
            "range": "+/- 15.931",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 336.024,
            "range": "+/- 1.657",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2143.452,
            "range": "+/- 7.294",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 236.707,
            "range": "+/- 0.893",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 44552.664,
            "range": "+/- 232.1",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 6241.16,
            "range": "+/- 38.161",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 384.372,
            "range": "+/- 1.493",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.372,
            "range": "+/- 0.058",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 94.247,
            "range": "+/- 0.265",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 18.09,
            "range": "+/- 0.074",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1536.688,
            "range": "+/- 4.018",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 29.609,
            "range": "+/- 0.048",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1166.883,
            "range": "+/- 4.686",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 8117.615,
            "range": "+/- 32.032",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 525.432,
            "range": "+/- 1.143",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 494500.116,
            "range": "+/- 1308.606",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 7811.083,
            "range": "+/- 25.421",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 169.096,
            "range": "+/- 0.789",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 1977.486,
            "range": "+/- 8.541",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 78.722,
            "range": "+/- 0.279",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 31316.583,
            "range": "+/- 78.215",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1341.935,
            "range": "+/- 6.703",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1180086.897,
            "range": "+/- 4396.682",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 18734.707,
            "range": "+/- 63.577",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 467.121,
            "range": "+/- 1.243",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 4848.407,
            "range": "+/- 28.136",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 176.936,
            "range": "+/- 0.411",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 73210.289,
            "range": "+/- 175.518",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 3812.131,
            "range": "+/- 43.709",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 4198.701,
            "range": "+/- 22.542",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0decbe3f23b9a3682a00f40051d4665e92b9899e",
          "message": "chore: keep this project's target dir, drop the obsolete check-ws alias (#84)\n\n`.cargo/config.toml` held a `check-ws` alias that ran `cargo check\n--workspace` with `--exclude hv1-core --exclude hv1-boot`, because\n`bootloader`'s build script passes `-Zbuild-std` and stable cargo rejects\nit, so a plain `cargo check --workspace` failed on the toolchain this\nrepository pins. That was issue #57.\n\nThe workspace now excludes `crates/hv1-boot` outright. CI runs exactly\n`cargo check --workspace --all-targets`, with no excludes, on stable, and\nso does this host:\n\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 56.18s\n\nThe alias would now be a slower way to check less, so it goes rather than\nstaying as advice that no longer holds.\n\nIn its place, `[build] target-dir = \"target\"`. A `~/.cargo/config.toml`\nthat points every project at one shared target directory is a reasonable\nway to keep a disk from filling, but it is the wrong default here: this\nbuild is large and worth keeping warm, and cargo takes an exclusive lock\non a target directory, so the projects most likely to be built\nconcurrently are the ones that most want their own. `target` is cargo's\nown default; this only asserts it against a machine-wide override.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T20:08:11-07:00",
          "tree_id": "20ffd26058f9161b7eff906521bc19b9f210f781",
          "url": "https://github.com/nervosys/HyperMachine/commit/0decbe3f23b9a3682a00f40051d4665e92b9899e"
        },
        "date": 1788405946607,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 329.776,
            "range": "+/- 0.556",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2047.546,
            "range": "+/- 7.876",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 405.752,
            "range": "+/- 0.763",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 672.213,
            "range": "+/- 4.696",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 248.163,
            "range": "+/- 1.029",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9031.911,
            "range": "+/- 58.818",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 373.655,
            "range": "+/- 1.008",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2401.523,
            "range": "+/- 6.114",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 297.495,
            "range": "+/- 0.605",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 735.843,
            "range": "+/- 3.302",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 283.845,
            "range": "+/- 1.644",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13884.066,
            "range": "+/- 47.448",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 1671.289,
            "range": "+/- 6.579",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 67.869,
            "range": "+/- 0.216",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 78.894,
            "range": "+/- 0.422",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 851.621,
            "range": "+/- 1.833",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1371.965,
            "range": "+/- 4.673",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 454.959,
            "range": "+/- 0.72",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 581.403,
            "range": "+/- 0.761",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 746.173,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8989.797,
            "range": "+/- 49.13",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 334.582,
            "range": "+/- 1.502",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2266.465,
            "range": "+/- 8.663",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 231.409,
            "range": "+/- 0.426",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 47301.787,
            "range": "+/- 157.812",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 9202.681,
            "range": "+/- 64.202",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 305.351,
            "range": "+/- 0.656",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 11.667,
            "range": "+/- 0.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 77.101,
            "range": "+/- 0.292",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 16.369,
            "range": "+/- 0.199",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1219.831,
            "range": "+/- 3.194",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 23.15,
            "range": "+/- 0.102",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1219.531,
            "range": "+/- 7.444",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 10928.182,
            "range": "+/- 64.028",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 573.906,
            "range": "+/- 2.868",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 538804.279,
            "range": "+/- 1058.783",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8546.373,
            "range": "+/- 37.383",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 204.15,
            "range": "+/- 1.66",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2115.07,
            "range": "+/- 6.519",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 95.298,
            "range": "+/- 0.675",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 34074.154,
            "range": "+/- 166.889",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1691.745,
            "range": "+/- 10.004",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1320976.082,
            "range": "+/- 4195.657",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 21518.405,
            "range": "+/- 131.333",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 568.855,
            "range": "+/- 3.856",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 5694.431,
            "range": "+/- 35.116",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 213.328,
            "range": "+/- 1.16",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 86853.11,
            "range": "+/- 504.377",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 4756.787,
            "range": "+/- 14.339",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5859.996,
            "range": "+/- 42.219",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "347c7453cc5b99c34f9ac42150be125e2a51a996",
          "message": "feat(pci): connect the PCI model to a port a guest can read\n\nThe `pci` module models a root complex, buses, config space and\ncapabilities across roughly 3,900 lines. Nothing registered any of it\nagainst an I/O port. `PciRootComplex` appeared exactly once outside its\nown module -- in the re-export list in lib.rs -- so a guest's first\nconfiguration probe fell through to the unhandled-port path, read 0xff,\nand concluded the machine has no PCI bus at all.\n\nNothing hung, which is why it went unnoticed. A kernel that finds nothing\nat 0xCF8 does not fail; it decides there is no bus and boots on. Every\ndevice behind PCI was invisible rather than broken, and the model behind\nit was as complete as it looked and as unreachable as it was.\n\nThis adds the Configuration Space Access Mechanism from PCI 3.0\n§3.2.2.3.2 as an ordinary Device over ports 0xCF8..=0xCFF, and puts it in\n`Machine::legacy_pc()` alongside the UART, RTC and i8042.\n\nTwo details that are easy to get wrong and are tested rather than\nasserted in a comment:\n\n  - The byte lane for a narrow access comes from the port, not from the\n    latched address. A guest reads the one-byte header type at register\n    0x0C with a byte access to 0xCFF; aliasing every narrow access to the\n    low byte of the dword would hand it the cache line size instead.\n\n  - A byte write patches its own bytes and leaves the rest of the dword\n    alone. Config space is written a dword at a time, so the naive\n    implementation clears three neighbouring registers on every byte\n    write -- the same defect the serial port had.\n\nWith bit 31 clear the mechanism is idle: reads give all ones and writes\ngo nowhere, rather than landing on device 0 of bus 0.\n\nThis is the prerequisite for a virtio-pci transport (roadmap C4), which\nis what lets a stock distribution kernel find a device without the\n`virtio_mmio.device=` argument a custom build needs today. It does not\ndecode BARs or route memory accesses; it answers configuration cycles,\nwhich is what enumeration consists of.\n\n`cargo test -p hv2-core` passes 2,189 tests, clippy is silent with\n`-D warnings`, and the new machine-level test drives the same write-then-\nread sequence a guest does rather than checking the port is mapped.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-02T20:30:52-07:00",
          "tree_id": "64bdb28ca53148188882c93b49f31ca526212bd3",
          "url": "https://github.com/nervosys/HyperMachine/commit/347c7453cc5b99c34f9ac42150be125e2a51a996"
        },
        "date": 1788407125226,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 530.539,
            "range": "+/- 2.295",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3393.512,
            "range": "+/- 35.208",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 403.662,
            "range": "+/- 0.978",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1018.733,
            "range": "+/- 5.758",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 373.798,
            "range": "+/- 1.294",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12528.289,
            "range": "+/- 48.364",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 616.684,
            "range": "+/- 1.639",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3875.632,
            "range": "+/- 13.446",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 474.358,
            "range": "+/- 1.95",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1171.493,
            "range": "+/- 4.505",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 442.749,
            "range": "+/- 2.633",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11705.379,
            "range": "+/- 77.225",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2635.681,
            "range": "+/- 3.934",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.522,
            "range": "+/- 0.206",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.223,
            "range": "+/- 0.307",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1209.131,
            "range": "+/- 2.45",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1943.351,
            "range": "+/- 2.377",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 664.675,
            "range": "+/- 3.426",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 860.549,
            "range": "+/- 2.091",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1054.841,
            "range": "+/- 1.45",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11981.908,
            "range": "+/- 6.525",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 498.093,
            "range": "+/- 0.953",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3244.429,
            "range": "+/- 3.613",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 358.858,
            "range": "+/- 0.604",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76719.663,
            "range": "+/- 682.008",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11102.709,
            "range": "+/- 63.068",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 401.223,
            "range": "+/- 2.059",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.369,
            "range": "+/- 0.051",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 108.124,
            "range": "+/- 0.499",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.869,
            "range": "+/- 0.087",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1600.026,
            "range": "+/- 5.59",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.973,
            "range": "+/- 0.076",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2052.711,
            "range": "+/- 10.152",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17095.231,
            "range": "+/- 274.495",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.408,
            "range": "+/- 0.51",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 771792.989,
            "range": "+/- 4059.302",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11837.908,
            "range": "+/- 10.414",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 279.867,
            "range": "+/- 0.3",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3045.494,
            "range": "+/- 3.313",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 138.997,
            "range": "+/- 0.183",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 47127.081,
            "range": "+/- 56.332",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2880.072,
            "range": "+/- 40.267",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2439720.318,
            "range": "+/- 61547.577",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 40545.665,
            "range": "+/- 748.354",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 957.46,
            "range": "+/- 15.638",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 10426.17,
            "range": "+/- 165.843",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 360.043,
            "range": "+/- 7.496",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 141483.964,
            "range": "+/- 1260.293",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6728.55,
            "range": "+/- 34.418",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8178.306,
            "range": "+/- 89.851",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "a44e2fa4f0aafd340979ebdf2af5cd8b2a78252f",
          "message": "feat(pci): a modern virtio-pci transport over the existing device trait\n\nSecond half of roadmap C4. The MMIO transport works, but only for a guest\nthat was told where to look: the address arrives on the command line as\n`virtio_mmio.device=4K@0xd0000000:5`, and the kernel must have been built\nwith CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES. A stock cloud image has neither,\nso it boots and reports no device rather than failing visibly.\n\nThis implements the modern virtio-pci layout -- common configuration, ISR,\nnotification and device-specific config, one page apart in a BAR, with the\nvendor-specific capability chain that says where each one is.\n\nNothing in virtio_vsock or virtio_blk had to change. `VirtioMmioDevice`\nnames queues, features, config space and a notify callback, none of which\nare transport-specific; the name is historical rather than descriptive, so\nboth transports drive the same devices.\n\nDetails that are tested rather than asserted in a comment, because each is\ninvisible until a real driver hits it:\n\n  - The queue a notification refers to is its address, not the value\n    written. The driver computes notify_base + queue_notify_off *\n    multiplier, so a multiplier of zero collapses every queue onto one\n    address and the device cannot tell them apart.\n\n  - Reading the ISR is the acknowledgement -- there is no separate ACK\n    register as in MMIO -- so the line is deasserted there. Holding it\n    would re-enter the handler forever; pulsing instead of asserting would\n    lose interrupts between deliveries, which is the same defect the MMIO\n    transport documents.\n\n  - The MSI-X vector registers read back 0xFFFF rather than what a driver\n    wrote. No MSI-X capability is offered, and a driver that reads back\n    its own vector concludes MSI-X works and then waits for interrupts\n    that never arrive.\n\n  - Features are reported in both 32-bit halves. VIRTIO_F_VERSION_1 is bit\n    32, so a device that answers only the low half tells the driver it is\n    a legacy device.\n\nTwo things about config space that cost a round of failing tests, and are\nworth recording: BAR reads are served from the `BarConfig`, not the raw\nbyte array, so bytes written directly are visible to nothing --\n`configure_bar` is the API, and the size mask it computes is also what\nmakes BAR sizing work. And `write_u32` is the guest write path, applying\nthe write mask that correctly drops writes to read-only registers;\nbuilding a device is not a guest write, so the capability chain uses the\nunmasked setters.\n\nNot implemented, and absent rather than stubbed so a driver falls back\ninstead of finding a structure that does nothing: MSI-X, and honouring a\nguest that reprograms the BAR.\n\n13 tests, including the bring-up sequence from virtio 1.2 3.1.1 driven in\norder. `cargo test -p hv2-core` passes 2,202; clippy is silent with\n-D warnings.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-02T20:58:23-07:00",
          "tree_id": "8f44e8ef93045fee60c0e0c68fb2534964c6246f",
          "url": "https://github.com/nervosys/HyperMachine/commit/a44e2fa4f0aafd340979ebdf2af5cd8b2a78252f"
        },
        "date": 1788409018972,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 632.54,
            "range": "+/- 7.232",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 4355.765,
            "range": "+/- 41.055",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 484.778,
            "range": "+/- 3.603",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1227.767,
            "range": "+/- 11.543",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 414.034,
            "range": "+/- 8.581",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12422.825,
            "range": "+/- 35.356",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 643.101,
            "range": "+/- 5.772",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4058.281,
            "range": "+/- 50.072",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 491.384,
            "range": "+/- 3.927",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1219.076,
            "range": "+/- 6.828",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 449.736,
            "range": "+/- 2.935",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 14033.094,
            "range": "+/- 122.534",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2805.11,
            "range": "+/- 8.949",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 102.979,
            "range": "+/- 1.466",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.576,
            "range": "+/- 0.657",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1250.148,
            "range": "+/- 4.428",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1990.29,
            "range": "+/- 6.446",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 698.383,
            "range": "+/- 3.808",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 872.754,
            "range": "+/- 2.74",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1065.694,
            "range": "+/- 2.531",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12128.638,
            "range": "+/- 64.76",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 510.891,
            "range": "+/- 2.587",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3271.434,
            "range": "+/- 5.957",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 369.037,
            "range": "+/- 1.96",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73751.236,
            "range": "+/- 155.703",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11014.26,
            "range": "+/- 87.716",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 403.119,
            "range": "+/- 1.319",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.615,
            "range": "+/- 0.228",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.324,
            "range": "+/- 0.586",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.753,
            "range": "+/- 0.161",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1621.396,
            "range": "+/- 5.548",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 37.46,
            "range": "+/- 0.323",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1977.539,
            "range": "+/- 7.632",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15305.816,
            "range": "+/- 172.302",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 837.569,
            "range": "+/- 0.921",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 1177568,
            "range": "+/- 165858.653",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11768.744,
            "range": "+/- 5.224",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.969,
            "range": "+/- 0.594",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3030.856,
            "range": "+/- 3.876",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 142.522,
            "range": "+/- 0.599",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 47249.669,
            "range": "+/- 65.841",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2657.501,
            "range": "+/- 30.175",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2316073.409,
            "range": "+/- 21739.539",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 37217.998,
            "range": "+/- 377.962",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 938.895,
            "range": "+/- 11.355",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9465.173,
            "range": "+/- 94.181",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 407.004,
            "range": "+/- 7.737",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 146468.224,
            "range": "+/- 1522.563",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6754.647,
            "range": "+/- 35.263",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7488.217,
            "range": "+/- 22.603",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "1dee1c3871f683ccc43622beec2e5d64e14a8511",
          "message": "feat(vm): attach a vsock device a stock kernel can find by itself\n\nCompletes roadmap C4. `attach_vsock_pci` puts a virtio-vsock device on the\nguest's PCI bus: configuration space into the root complex the 0xCF8\nwindow reads, the BAR window registered as an MMIO region, and the\ninterrupt line reported so the driver knows what to unmask.\n\nThe difference from `attach_vsock` is discovery, not function. Over MMIO\nthe guest is told where to look, on the kernel command line, and only a\nkernel built with CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES can act on it. Here\nthe guest walks a bus it already knows how to walk and binds virtio_pci.\nNothing is added to the command line, deliberately, and a test asserts\nthat -- an argument would mean the device was not discoverable after all.\n\nThree supporting changes:\n\n  - `AttachedVsock` holds a `VsockTransport` enum rather than the MMIO\n    transport specifically. The only thing the host side asks of a\n    transport is that it can raise the used-queue interrupt after\n    publishing, so the two are interchangeable behind one method.\n\n  - `Machine::legacy_pc_with_pci_root` shares a caller's root complex.\n    Attaching a PCI device means adding configuration space to the same\n    root complex the guest enumerates, and there was no way to reach the\n    one `legacy_pc` built for itself.\n\n  - `VM::pci_root` exposes it, for the same reason: a caller adding its\n    own PCI device needs somewhere real to add it to.\n\nThe interrupt line matters more than it looks. Without `set_interrupt_line`\nand `set_interrupt_pin` a driver binds, programs its queues, and then waits\non an interrupt nobody raises -- which from inside the guest is\nindistinguishable from a device that never answers.\n\nThe tests read through the same port a guest uses -- write CONFIG_ADDRESS,\nread CONFIG_DATA -- rather than inspecting the root complex directly.\nConfiguration space that no port exposes is the exact failure this change\nexists to fix, and a test that reached past the port could not tell the\ntwo apart. They attach the machine model rather than calling `provision`,\nwhich needs a hypervisor the host running the tests may not have.\n\n`cargo test -p hv2-core` passes 2,206; hv2-agent and hv2-api pass\nunchanged; `cargo check --workspace --all-targets` is clean and clippy is\nsilent with -D warnings.\n\nStill MMIO-only until someone asks for PCI: nothing changes for existing\ncallers, and no guest has yet booted against this on hardware.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-02T21:19:07-07:00",
          "tree_id": "5a5e10f094a1bd86c0d02573ddbc0fa126d22842",
          "url": "https://github.com/nervosys/HyperMachine/commit/1dee1c3871f683ccc43622beec2e5d64e14a8511"
        },
        "date": 1788409995760,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 531.829,
            "range": "+/- 2.59",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3428.025,
            "range": "+/- 27.621",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 423.094,
            "range": "+/- 4.735",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1014.793,
            "range": "+/- 4.935",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 370.152,
            "range": "+/- 0.788",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12867.6,
            "range": "+/- 47.146",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 615.315,
            "range": "+/- 3.435",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3824.31,
            "range": "+/- 17.681",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 498.046,
            "range": "+/- 4.964",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1186.5,
            "range": "+/- 3.377",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 452.116,
            "range": "+/- 3.251",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13850.006,
            "range": "+/- 114.392",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2744.822,
            "range": "+/- 6.942",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 108.374,
            "range": "+/- 1.672",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 121.694,
            "range": "+/- 1.332",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1219.668,
            "range": "+/- 4.117",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1972.742,
            "range": "+/- 6.456",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 656.323,
            "range": "+/- 4.934",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 831.502,
            "range": "+/- 1.55",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1053.709,
            "range": "+/- 0.972",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12182.503,
            "range": "+/- 26.894",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 498.808,
            "range": "+/- 0.828",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3260.764,
            "range": "+/- 3.543",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 360.666,
            "range": "+/- 0.782",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73825.653,
            "range": "+/- 303.412",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11464.616,
            "range": "+/- 100.586",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 398.769,
            "range": "+/- 1.557",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.343,
            "range": "+/- 0.223",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 102.444,
            "range": "+/- 0.276",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.55,
            "range": "+/- 0.118",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1580.505,
            "range": "+/- 4.988",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.495,
            "range": "+/- 0.387",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2002.745,
            "range": "+/- 20.981",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16175.216,
            "range": "+/- 298.601",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 882.026,
            "range": "+/- 11.903",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 752420.672,
            "range": "+/- 2215.926",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11807.343,
            "range": "+/- 9.094",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.516,
            "range": "+/- 0.433",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3035.231,
            "range": "+/- 3.031",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 138.874,
            "range": "+/- 0.168",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46889.041,
            "range": "+/- 101.362",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2662.942,
            "range": "+/- 22.772",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2466402.545,
            "range": "+/- 30366.716",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 39650.173,
            "range": "+/- 650.266",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 980.322,
            "range": "+/- 12.067",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9322.873,
            "range": "+/- 66.899",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 335.702,
            "range": "+/- 3.36",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140320.904,
            "range": "+/- 365.206",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6826.075,
            "range": "+/- 62.438",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7509.172,
            "range": "+/- 52.513",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "0be69ef1b95935eb9d66a248ef097795e9a7d274",
          "message": "feat(container): translate an OCI spec into confinement the kernel enforces\n\nRoadmap E1, arrived at differently than planned. The decision was to make\nthe OCI module the sandbox backend. Reading it first showed that premise\nwas wrong in a way worth recording:\n\n  crates/hv2-core/src/container/   3,921 lines, 0 calls to libc\n  crates/hv2-sandbox/.../linux.rs               87 calls to libc\n\n`ContainerRuntime::start` returns `NotImplemented(\"starting a\ncontainer\")`. The module's own doc already said it \"does not run\nanything\" and pointed at hv2-sandbox. Meanwhile hv2-sandbox already\nimplements everything the module describes: CLONE_NEWNS/NEWNET/NEWPID/\nNEWIPC, pivot_root, cgroup v2 memory.max and pids.max, RLIMIT_CPU,\nPR_SET_NO_NEW_PRIVS.\n\nSo there was no backend to wire it to, because the backend already\nexisted and was the sandbox. Making the OCI types a backend would have\nmeant rebuilding a working implementation behind a model that does\nnothing, and keeping two implementations of one thing in agreement\nforever.\n\nThis inverts it instead. The sandbox stays the backend; OCI becomes an\ninput format. `to_sandbox` turns a ContainerSpec into the SandboxSpec and\nSandboxCommand hv2-sandbox enforces -- the first path by which an OCI\nspecification in this codebase does anything at all.\n\nThe vocabularies are not the same size, and that is the whole risk. A\ntranslation that quietly ignored what it could not express would return\nconfinement weaker than the caller asked for -- seccomp filter gone, uid\nswitch gone, read-only path writable -- with nothing to say so, which is\nworse than refusing because the caller cannot find out. So every field is\neither translated or named in the error, and the error carries the OCI\nfield name rather than prose so a caller can act on it.\n\nRefused rather than approximated: seccomp, uid and gid mappings, user,\nUTS, cgroup and time namespaces, masked and read-only paths, block I/O, a\nterminal, non-root process.uid, joining an existing namespace, a\nrelocating bind mount, and a root without a mount namespace to apply it\nin. CPU is the one worth spelling out: OCI bounds a share of wall-clock\nper period, the sandbox bounds total CPU consumed, and mapping one onto\nthe other would be arithmetic without meaning.\n\nAll unsupported fields are reported at once. Reporting the first would\nmake a caller with four of them run four times to learn it cannot run at\nall.\n\nOne test checks the output against a Controls set enforcing everything,\nso a translation that produced a spec `reconcile` rejects would fail --\notherwise this would have moved the failure rather than removed it.\n\nhv2-core gains a dependency on hv2-sandbox. hv2-sandbox depends on\nnothing here, so the direction is acyclic.\n\n15 tests. `cargo test -p hv2-core` passes 2,221, clippy is silent with\n-D warnings, and `cargo check --workspace --all-targets` is clean.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T00:08:54-07:00",
          "tree_id": "4c70a3f3f3cf0c4ffc3bf4a66c762315589ad893",
          "url": "https://github.com/nervosys/HyperMachine/commit/0be69ef1b95935eb9d66a248ef097795e9a7d274"
        },
        "date": 1788420295715,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 493.916,
            "range": "+/- 2.466",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 4410.635,
            "range": "+/- 7.41",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 380.223,
            "range": "+/- 0.783",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1005.533,
            "range": "+/- 4.023",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 337.33,
            "range": "+/- 1.639",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12489.937,
            "range": "+/- 146.145",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 574.052,
            "range": "+/- 4.111",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4782.81,
            "range": "+/- 33.384",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 447.101,
            "range": "+/- 4.204",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1108.38,
            "range": "+/- 3.501",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 418.191,
            "range": "+/- 4.114",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12755.383,
            "range": "+/- 44.608",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2615.907,
            "range": "+/- 19.134",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.925,
            "range": "+/- 0.583",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 118.027,
            "range": "+/- 1.183",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1172.471,
            "range": "+/- 6.725",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1891.102,
            "range": "+/- 14.917",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 604.828,
            "range": "+/- 2.756",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 803.567,
            "range": "+/- 5.544",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 947.17,
            "range": "+/- 3.004",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10660.312,
            "range": "+/- 8.286",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 455.327,
            "range": "+/- 1.637",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3012.596,
            "range": "+/- 19.601",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 334.219,
            "range": "+/- 2.517",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73523.025,
            "range": "+/- 399.489",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13258.203,
            "range": "+/- 179.008",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 381.143,
            "range": "+/- 8.675",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 20.486,
            "range": "+/- 0.303",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 96.2,
            "range": "+/- 0.556",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.37,
            "range": "+/- 0.218",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1392.541,
            "range": "+/- 2.993",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.781,
            "range": "+/- 0.189",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2091.011,
            "range": "+/- 41.237",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18263.656,
            "range": "+/- 438.195",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 732.36,
            "range": "+/- 1.603",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662695.797,
            "range": "+/- 392.307",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10487.109,
            "range": "+/- 24.924",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.435,
            "range": "+/- 0.384",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2699.745,
            "range": "+/- 2.884",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 127.992,
            "range": "+/- 0.506",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41475.787,
            "range": "+/- 31.449",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2298.525,
            "range": "+/- 19.168",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2050318.36,
            "range": "+/- 11986.949",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31552.591,
            "range": "+/- 141.031",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 793.038,
            "range": "+/- 1.807",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8117.444,
            "range": "+/- 33.254",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 296.461,
            "range": "+/- 1.739",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 131905.775,
            "range": "+/- 3674.791",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7416.811,
            "range": "+/- 51.395",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8566.955,
            "range": "+/- 81.204",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "86380bb79e2d3eb1d3d235997b179ba8387bd283",
          "message": "feat(bench): measure cold start, the metric a sandbox is judged on\n\nCubeSandbox publishes 60ms cold start at single concurrency, and at 50\nconcurrent creations 67ms average with P95 90ms and P99 137ms. This\nrepository had no comparable number. `vm_bench`, whose results are\npublished to gh-pages, measures guest-memory allocation and snapshot\nserialisation -- neither of which is a boot.\n\nSo there was no way to tell whether this is faster or slower than anything,\nand no way to make a claim about it that could be checked. This is the\ninstrument, in the same shape as the published figures: percentiles at a\ngiven concurrency, decomposed by phase, because \"slower than 60ms\" is not\nactionable and \"provisioning is 80% of it\" is.\n\n  build    nothing               -> a configured VM with a backend handle\n  channel  that                  -> a vsock device attached\n  launch   that                  -> the vCPU running guest code\n  ready    that                  -> the guest agent answering a ping\n\n`ready` is the one comparable to a published cold start. A VM whose vCPU is\nrunning but whose guest has not finished booting is not a sandbox anyone can\nuse, so stopping at `launch` would flatter the number by leaving out the\npart that takes longest.\n\nIt refuses to print a number it did not measure. With no hypervisor backend\nit says so and exits non-zero; with no guest image it reports the phases it\ncould measure and names the one it could not. A benchmark that silently\ndegrades to timing less work is how a project comes to believe it is fast.\n\nIt also says so when built without --release, because a debug figure is not\na cold start anyone would deploy.\n\nVerified by running it: on this host it correctly declines to report,\nbecause Windows Hypervisor Platform fails at `Failed to set processor\ncount: HRESULT 0x80370302` and /dev/kvm under WSL2 is not accessible to\nthis user. That is the intended behaviour for an absent backend, and it is\nalso the current honest answer to \"how fast is it\" -- unmeasured.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T07:39:41-07:00",
          "tree_id": "a22338909c66b3f97f76d690a42c4ed57b41286d",
          "url": "https://github.com/nervosys/HyperMachine/commit/86380bb79e2d3eb1d3d235997b179ba8387bd283"
        },
        "date": 1788447556479,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 520.466,
            "range": "+/- 2.477",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3445.431,
            "range": "+/- 17.925",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 386.027,
            "range": "+/- 1.903",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1028.627,
            "range": "+/- 7.252",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 349.192,
            "range": "+/- 1.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12753.2,
            "range": "+/- 81.29",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 561.903,
            "range": "+/- 1.224",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3627.384,
            "range": "+/- 23.038",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 439.218,
            "range": "+/- 2.918",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1114.619,
            "range": "+/- 3.045",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 394.56,
            "range": "+/- 0.619",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11563.127,
            "range": "+/- 68.73",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2565.879,
            "range": "+/- 22.096",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.734,
            "range": "+/- 0.333",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 112.997,
            "range": "+/- 0.24",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1097.527,
            "range": "+/- 1.481",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1768.734,
            "range": "+/- 5.442",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 601.873,
            "range": "+/- 2.597",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 765.27,
            "range": "+/- 2.066",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 952.182,
            "range": "+/- 3.137",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10734.233,
            "range": "+/- 15.705",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 462.68,
            "range": "+/- 2.5",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2923.457,
            "range": "+/- 10.955",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 322.774,
            "range": "+/- 0.875",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72282.904,
            "range": "+/- 309.883",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12857.724,
            "range": "+/- 103.206",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.531,
            "range": "+/- 0.803",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.304,
            "range": "+/- 0.104",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 96.736,
            "range": "+/- 0.402",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.597,
            "range": "+/- 0.105",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1404.736,
            "range": "+/- 5.687",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.433,
            "range": "+/- 0.276",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1998.011,
            "range": "+/- 20.152",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17368.514,
            "range": "+/- 196.1",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 731.895,
            "range": "+/- 0.488",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662506.305,
            "range": "+/- 447.279",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10502.11,
            "range": "+/- 18.58",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 248.38,
            "range": "+/- 0.352",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2686.767,
            "range": "+/- 2.953",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.982,
            "range": "+/- 0.352",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41539.319,
            "range": "+/- 103.765",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2311.965,
            "range": "+/- 14.973",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2013731.84,
            "range": "+/- 12438.949",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 36418.535,
            "range": "+/- 696.288",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 800.806,
            "range": "+/- 5.506",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8687.724,
            "range": "+/- 121.508",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 298.154,
            "range": "+/- 1.311",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 130021.607,
            "range": "+/- 1473.201",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7639.438,
            "range": "+/- 89.287",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8956.103,
            "range": "+/- 137.411",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "d48c872d5022fd85c927a3d270afd1145a2d846d",
          "message": "perf(kvm): stop memsetting the guest's RAM, which was the whole cold start\n\nMeasured, not guessed. The cold-start harness added in 86380bb reports:\n\n  before   build 1.44ms   channel 0.15ms   launch 998.79ms   total 1000.37ms\n  after    build 1.17ms   channel 0.10ms   launch   0.84ms   total    2.11ms\n\n`launch` was 99.8% of a cold start, and scaled with guest memory -- 944ms at\n1 GiB, 1648ms at 2 GiB. That is the shape of touching every page, not of\nwork.\n\nThe cause is one line and one alignment. `create_vm` allocated the guest's\nRAM with\n\n    let layout = Layout::from_size_align(memory_size, 4096)?;\n    let ptr = std::alloc::alloc_zeroed(layout);\n\nRust's `alloc_zeroed` forwards to `calloc` only when the alignment is at\nmost `MIN_ALIGN`, which is 16 on x86-64. KVM needs a page-aligned address,\nso 4096 took the other branch: `aligned_alloc` followed by\n`write_bytes(ptr, 0, size)`. A full memset of the guest's RAM, to zero\nmemory the kernel already guarantees is zero.\n\nMeasured on this host to tell the two apart, because they are easy to\nconfuse:\n\n    calloc(1 GiB)    0.0 ms\n    memalign(1 GiB)  0.0 ms\n    memset 1 GiB   848.1 ms   (1.27 GB/s)\n\n848ms against a measured 944ms launch. The memset was the cold start.\n\nNow an anonymous `mmap` with MAP_NORESERVE, which the kernel zeroes lazily\non first touch. `munmap` in `Drop` and on both error paths, because freeing\nan mmap through the Rust allocator would be undefined behaviour.\n\nThis decides density as much as latency. Writing every page materialises\nthe whole allocation immediately, so a 1 GiB VM cost 1 GiB of host RAM\nbefore the guest executed one instruction. Mapped lazily, a VM costs what\nits guest has actually touched -- which is the precondition for the\n\"thousands per node\" figure this is being measured against.\n\nVerified on Linux under WSL2 with real KVM (API 12, 24 vCPUs): 2,167 tests\npass and clippy is silent with -D warnings. `devices::timer::tests::\ntest_timer_frequency` fails both with and without this change (16 and 17\nticks against an expected 19), so it is a pre-existing wall-clock-dependent\ntest, not a regression from this.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T09:05:25-07:00",
          "tree_id": "87937b055ed932d4fac27482ae385eb4d822e3c9",
          "url": "https://github.com/nervosys/HyperMachine/commit/d48c872d5022fd85c927a3d270afd1145a2d846d"
        },
        "date": 1788452717679,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 563.697,
            "range": "+/- 5.33",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3403.961,
            "range": "+/- 14.194",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 430.458,
            "range": "+/- 5.958",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1092.404,
            "range": "+/- 10.88",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 385.885,
            "range": "+/- 3.385",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12856.816,
            "range": "+/- 288.112",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 614.245,
            "range": "+/- 7.901",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4825.797,
            "range": "+/- 21.649",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 464.353,
            "range": "+/- 3.229",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1118.3,
            "range": "+/- 8.232",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 423.862,
            "range": "+/- 3.694",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11688.263,
            "range": "+/- 97.376",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2425.149,
            "range": "+/- 15.736",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 115.722,
            "range": "+/- 3.116",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 119.288,
            "range": "+/- 0.75",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1082.569,
            "range": "+/- 5.762",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2124.088,
            "range": "+/- 123.057",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 581.592,
            "range": "+/- 1.4",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 753.827,
            "range": "+/- 4.805",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 945.957,
            "range": "+/- 2.808",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10681.586,
            "range": "+/- 10.108",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 438.641,
            "range": "+/- 1.005",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2894.196,
            "range": "+/- 3.459",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 318.514,
            "range": "+/- 0.543",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72286.915,
            "range": "+/- 363.601",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12795.918,
            "range": "+/- 62.94",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 358.085,
            "range": "+/- 0.974",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.255,
            "range": "+/- 0.237",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.382,
            "range": "+/- 0.344",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.49,
            "range": "+/- 0.08",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1433.48,
            "range": "+/- 7.533",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.262,
            "range": "+/- 0.264",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1935.42,
            "range": "+/- 16.562",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16818.677,
            "range": "+/- 151.039",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.466,
            "range": "+/- 0.817",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 665190.467,
            "range": "+/- 539.58",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10454.63,
            "range": "+/- 9.093",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.751,
            "range": "+/- 0.633",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2685.885,
            "range": "+/- 4.08",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.275,
            "range": "+/- 0.277",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41585.122,
            "range": "+/- 74.705",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2265.883,
            "range": "+/- 11.202",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1992276.885,
            "range": "+/- 7594.796",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31392.485,
            "range": "+/- 130.153",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 801.005,
            "range": "+/- 3.219",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8126.565,
            "range": "+/- 42.601",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 298.218,
            "range": "+/- 0.978",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124450.443,
            "range": "+/- 640.096",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7658.407,
            "range": "+/- 78.141",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8338.397,
            "range": "+/- 66.215",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "dc3d276e788b578233186c94085fc3dab54e454a",
          "message": "fix(bench): stop the harness measuring its own garbage\n\nIt never stopped the VMs it created. A vCPU left running does not idle, it\nspins, so every iteration after the first measured a busier machine -- and\na run that could not reach the guest agent left one spinning per iteration.\nObserved directly: five abandoned VMs at 498% CPU, still running ten\nminutes after the measurement they belonged to.\n\nEach creation is now stopped whatever happened, including before returning\na failure, so a failed iteration costs the machine nothing afterwards.\n\nThe ready timeout drops from 30s to 10s and takes --ready-timeout-secs. At\n30s a run that cannot reach the guest spends its time waiting rather than\ntelling anyone, which is the same fault in a different place.\n\nFirst full measurement, with a 6.6.52 kernel and an initramfs running\nhv2-guest-agentd, 8 iterations on real KVM:\n\n  build      avg   0.48ms\n  channel    avg   0.31ms\n  launch     avg  24.75ms\n  ready      avg 988.29ms\n  running    avg  25.54ms\n  usable     avg 1013.84ms   P50 1018.10ms   P95 1043.91ms\n\nSo a usable sandbox takes about a second, against the 60ms CubeSandbox\npublishes, and 97.5% of it is the guest kernel booting. Everything this\nproject controls -- creating the VM, mapping memory, attaching a channel,\nloading the image -- is 25ms of it.\n\nThat is the honest starting point, and it says where the next work is: not\nin the VMM.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T09:28:11-07:00",
          "tree_id": "d2944cec324dea1de11e6d41c205f20f8f265520",
          "url": "https://github.com/nervosys/HyperMachine/commit/dc3d276e788b578233186c94085fc3dab54e454a"
        },
        "date": 1788454079608,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 509.224,
            "range": "+/- 2.107",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3752.573,
            "range": "+/- 61.324",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 376.96,
            "range": "+/- 1.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1072.095,
            "range": "+/- 18.358",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 344.389,
            "range": "+/- 1.27",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13184.534,
            "range": "+/- 137.238",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 576.314,
            "range": "+/- 4.268",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6029.251,
            "range": "+/- 28.003",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 435.269,
            "range": "+/- 1.031",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1112.971,
            "range": "+/- 5.114",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 406.469,
            "range": "+/- 2.754",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12724.122,
            "range": "+/- 38.384",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2562.256,
            "range": "+/- 18.362",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.639,
            "range": "+/- 0.773",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.502,
            "range": "+/- 0.649",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1123.442,
            "range": "+/- 1.956",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1826.823,
            "range": "+/- 3.387",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 597.99,
            "range": "+/- 1.749",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 778.819,
            "range": "+/- 4.147",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 940.508,
            "range": "+/- 1.546",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10662.483,
            "range": "+/- 24.226",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 475.389,
            "range": "+/- 4.785",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3000.866,
            "range": "+/- 46.341",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 338.318,
            "range": "+/- 1.993",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74979.016,
            "range": "+/- 659.691",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12804.378,
            "range": "+/- 82.241",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 355.878,
            "range": "+/- 0.965",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.497,
            "range": "+/- 0.108",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.591,
            "range": "+/- 0.381",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.373,
            "range": "+/- 0.085",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1415.534,
            "range": "+/- 3.946",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.78,
            "range": "+/- 0.068",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1962.124,
            "range": "+/- 12.633",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16586.052,
            "range": "+/- 59.246",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 738.375,
            "range": "+/- 2.038",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662994.958,
            "range": "+/- 895.204",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10511.721,
            "range": "+/- 30.454",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.382,
            "range": "+/- 0.427",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2708.197,
            "range": "+/- 12.35",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 130.533,
            "range": "+/- 0.687",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41518.704,
            "range": "+/- 35.464",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2439.813,
            "range": "+/- 40.721",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2164059.478,
            "range": "+/- 26488.552",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 32988.125,
            "range": "+/- 303.619",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 885.573,
            "range": "+/- 10.143",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8251.097,
            "range": "+/- 58.195",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 317.731,
            "range": "+/- 1.496",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 134469.508,
            "range": "+/- 1689.965",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7292.715,
            "range": "+/- 30.606",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8422.546,
            "range": "+/- 50.185",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "b11283761105889022a6a2314788306f6a6526d0",
          "message": "feat(examples): boot a unikernel, and measure what having no kernel is worth\n\nThe unikernel pillar was the least evidenced of the four this project\nclaims. `BootSource::Raw` existed and its doc named unikernels, but the\nonly example that used it, `boot_probe`, loads\n`examples/guest_code/hello.bin` -- a path that does not exist in this\nrepository and never has. The `unikernel_*` tests in hv2-runtime validate\nboot-protocol structs; none of them boots anything.\n\nSo this boots one, on real KVM:\n\n  image         : 73 bytes, assembled in-process, entry 0x7c00\n  VM::new       :    0.079 ms\n  provision     :    2.011 ms  (cumulative)\n  launch        :    2.102 ms  (cumulative)\n  first output  :    3.279 ms  (cumulative)\n  console       : \"HYPERMACHINE UNIKERNEL\\n\"\n\nOver nine runs, first output lands between 2.58ms and 4.18ms, median\n2.67ms.\n\nThe number matters against the thing being measured. CubeSandbox publishes\n60ms to a usable sandbox. Booting Linux here measured 1,014ms, of which\n988ms was the guest kernel. A unikernel does not make that second faster;\nit does not have it. 2.67ms is what remains when there is no kernel to\nboot.\n\nThe security argument is the same fact from the other side. There is no\nscheduler, no init, no module loader, no filesystem and no syscall\nboundary, because there is nothing on the other side of one. 73 bytes of\nguest code, and every byte of it is the workload. For running a small\nspecialised agent under hardened isolation, the kernel that is not there\nis the attack surface that is not there.\n\nThe image is assembled in-process rather than shipped as a binary, so this\nhas no missing-asset failure mode -- the one that left `boot_probe` unable\nto run since it was written -- and a reader can check thirteen\ninstructions against the encoding table instead of trusting a blob.\n\nIt proves the path rather than asserting it. Every `out` leaves the guest,\nis decoded here, and lands in a device model, so a byte on the host\nconsole means the image loaded at the right address, the vCPU started in\nthe right mode at the right instruction, the I/O exit was decoded, and the\nport routed to the device that claims it. An empty console proves none of\nthat, which is why the console contents are printed rather than a success\nline, and why an empty one exits non-zero.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T09:56:41-07:00",
          "tree_id": "5d3f04a25dcc84bd777e61522abab0e10612d8b9",
          "url": "https://github.com/nervosys/HyperMachine/commit/b11283761105889022a6a2314788306f6a6526d0"
        },
        "date": 1788455458127,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 510.5,
            "range": "+/- 1.838",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3418.646,
            "range": "+/- 12.599",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 381.626,
            "range": "+/- 1.636",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 997.884,
            "range": "+/- 4.067",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 360.16,
            "range": "+/- 3.255",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12532.846,
            "range": "+/- 31.168",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 569.026,
            "range": "+/- 2.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4932.524,
            "range": "+/- 24.76",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 438.429,
            "range": "+/- 1.142",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1108.031,
            "range": "+/- 4.868",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 403.747,
            "range": "+/- 2.045",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11775.5,
            "range": "+/- 54.841",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2728.298,
            "range": "+/- 52.867",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 106.385,
            "range": "+/- 1.727",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.356,
            "range": "+/- 1.134",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1092.796,
            "range": "+/- 8.22",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1726.455,
            "range": "+/- 5.511",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 612.112,
            "range": "+/- 4.104",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 794.19,
            "range": "+/- 6.277",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 932.818,
            "range": "+/- 1.819",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10840.48,
            "range": "+/- 31.297",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 438.914,
            "range": "+/- 0.566",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2887.241,
            "range": "+/- 1.75",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 319.331,
            "range": "+/- 1.001",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76211.696,
            "range": "+/- 1149.442",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12975.018,
            "range": "+/- 197.392",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 369.65,
            "range": "+/- 2.853",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.514,
            "range": "+/- 0.13",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 99.866,
            "range": "+/- 0.712",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.264,
            "range": "+/- 0.222",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1422.607,
            "range": "+/- 5.302",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.438,
            "range": "+/- 0.363",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1938.74,
            "range": "+/- 15.667",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 19342.515,
            "range": "+/- 560.585",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.22,
            "range": "+/- 0.99",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 668913.853,
            "range": "+/- 2091.207",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11007.094,
            "range": "+/- 177.628",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 246.841,
            "range": "+/- 0.771",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2689.423,
            "range": "+/- 3.753",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 125.393,
            "range": "+/- 0.168",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 42123.482,
            "range": "+/- 126.696",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2255.627,
            "range": "+/- 8.819",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2000739.88,
            "range": "+/- 7007.831",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31578.044,
            "range": "+/- 152.489",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 798.68,
            "range": "+/- 3.62",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8494.19,
            "range": "+/- 116.104",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 300.097,
            "range": "+/- 2.369",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124816.78,
            "range": "+/- 403.952",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7611.877,
            "range": "+/- 111.822",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8372.444,
            "range": "+/- 86.704",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "b44022c3a09ef57801822aed38b7705fead185da",
          "message": "feat(examples): measure memory per sandbox, and find the density ceiling\n\nThe other number a sandbox is judged on. CubeSandbox publishes \"less than\n5MB of memory overhead\" per sandbox and rests \"thousands of instances per\nserver\" on it. This measures ours, and found one result far better than\ntheirs and one far worse.\n\nOverhead, 20 concurrent unikernel VMs, all 20 verified as having executed:\n\n  baseline RSS    3.07 MiB\n  after 20 VMs    5.97 MiB\n  growth          2.89 MiB total, 0.145 MiB per VM\n  marginal        0.137 MiB per additional VM\n\n0.14 MB against their 5 MB, and independent of guest size: 8 GiB guests\ncost 0.144 MiB each, the same as 1 GiB guests. 20 GiB of address space\nreserved across twenty guests, almost none of it resident. That\nindependence is the property their figure asserts, and it only became true\nwhen guest RAM stopped being memset -- before d48c872 a 1 GiB guest cost\n1 GiB of host RAM and none of this was measurable.\n\nRSS rather than virtual size, deliberately. A lazily mapped guest reserves\naddress space it does not occupy, and reporting the reservation would give\nthe flattering number for density and the wrong one.\n\nThen the bad result. Above roughly twenty concurrent VMs the process hangs\nat 0% CPU -- blocked, not busy. The ceiling tracks tokio worker threads:\n\n  4 workers    3 VMs ok, 6 hang\n  24 workers  20 VMs ok, 30 hang\n\nAbout one VM per runtime worker, because a running vCPU blocks a worker\ninside KVM_RUN. So concurrent sandboxes are capped by host CPU count: on a\n64-core machine that is roughly fifty, not thousands. Per-sandbox memory\nis not what limits density here; the executor is.\n\nThis codebase already knows the pattern. Interrupt delivery and the vsock\npump were both moved to dedicated OS threads, with the comment that \"the\nvCPU loop blocks a runtime worker inside KVM_RUN\". Everything around the\nvCPU was moved off the runtime. The vCPU was not.\n\nThe example keeps its VMs alive rather than dropping them, because the\nquestion is what N concurrent sandboxes cost, and checks each guest\nactually produced output -- a VM that never executed is not a sandbox and\nits cost is not an overhead figure.\n\nLinux only: it reads /proc/self/statm.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T10:42:19-07:00",
          "tree_id": "c138bb220bee6cc0b674c70d0676ce315bdd4102",
          "url": "https://github.com/nervosys/HyperMachine/commit/b44022c3a09ef57801822aed38b7705fead185da"
        },
        "date": 1788458442205,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 522.838,
            "range": "+/- 1.486",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3384.466,
            "range": "+/- 12.226",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 393.733,
            "range": "+/- 0.645",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1029.031,
            "range": "+/- 3.608",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 364.924,
            "range": "+/- 1.117",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12557.114,
            "range": "+/- 23.683",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 574.366,
            "range": "+/- 1.605",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 5982.826,
            "range": "+/- 17.75",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 445.176,
            "range": "+/- 0.988",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1126.156,
            "range": "+/- 5.485",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 414.807,
            "range": "+/- 1.261",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12822.485,
            "range": "+/- 24.794",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2500.275,
            "range": "+/- 17.332",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.755,
            "range": "+/- 0.612",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.007,
            "range": "+/- 0.789",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1163.211,
            "range": "+/- 7.307",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1871.916,
            "range": "+/- 11.634",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 591.028,
            "range": "+/- 2.266",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 779.764,
            "range": "+/- 4.41",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 990.57,
            "range": "+/- 7.682",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10649.818,
            "range": "+/- 6.492",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 451.633,
            "range": "+/- 1.873",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2920.859,
            "range": "+/- 6.878",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 321.964,
            "range": "+/- 1.02",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75652.894,
            "range": "+/- 331.373",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12712.412,
            "range": "+/- 102.525",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.028,
            "range": "+/- 0.554",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.828,
            "range": "+/- 0.249",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.846,
            "range": "+/- 0.618",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.619,
            "range": "+/- 0.281",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1412.664,
            "range": "+/- 3.349",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.213,
            "range": "+/- 0.705",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1955.548,
            "range": "+/- 14.163",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16923.243,
            "range": "+/- 150.929",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 734.502,
            "range": "+/- 1.669",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662691.709,
            "range": "+/- 397.433",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10465.163,
            "range": "+/- 4.492",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 248.211,
            "range": "+/- 0.838",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2691.802,
            "range": "+/- 1.839",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.417,
            "range": "+/- 0.156",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41476.488,
            "range": "+/- 28.358",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2274.021,
            "range": "+/- 14.29",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2001081.56,
            "range": "+/- 11306.404",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31275.03,
            "range": "+/- 111.669",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 796.379,
            "range": "+/- 2.877",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8135.494,
            "range": "+/- 37.452",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.129,
            "range": "+/- 1.342",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124124.556,
            "range": "+/- 447.128",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7460.331,
            "range": "+/- 56.616",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8309.668,
            "range": "+/- 44.07",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "f3ff4a9dfb3e2656763f3aa029ee8ddf97755500",
          "message": "refactor(vm): run every vCPU on its own thread, not only the pinned ones\n\nA vCPU with an affinity already got a dedicated OS thread with its own\ncurrent-thread runtime. A vCPU without one -- the default -- was a plain\n`tokio::spawn` onto the shared runtime, where `run_vcpu` blocks inside\n`KVM_RUN` until the guest exits. That occupies a runtime worker for as\nlong as the guest is running rather than yielding, which is what the rest\nof this file already moved away from: interrupt delivery and the vsock\npump each took a dedicated thread, both commented with that exact\nobservation. Everything around the vCPU was moved off the runtime; the\nvCPU was not.\n\nThe two paths are now one, differing only in whether the thread is pinned.\nA blocking ioctl wants a thread of its own regardless: the kernel is the\nscheduler, the thread is descheduled inside the ioctl rather than\nspinning, and an idle guest costs a parked thread.\n\nIt did not fix what I hoped it would fix, and the measurement says so.\nConcurrent VMs still stop at exactly the host core count:\n\n  23 VMs   0.139 MiB per VM, all guests ran\n  24 VMs   hangs at 0% CPU     (nproc = 24)\n\nSo a worker is still consumed per VM, by something other than the vCPU\nloop -- and it is not KVM: 400 bare `KVM_CREATE_VM` fds open on this host\nwith no error. The progress output added to memory_overhead narrows it,\nshowing every VM created and the process blocking inside the last one\nrather than partway through the set.\n\nKeeping the change anyway, because it is right on its own terms and\nremoves one real coupling between guest execution and the executor. The\nremaining one is still to find.\n\nVerified on Linux with KVM: 2,167 tests pass, clippy silent with\n-D warnings, and the unikernel still boots to first output in the same\nfew milliseconds. `test_timer_frequency` fails here as it does on a clean\ntree -- a pre-existing wall-clock-dependent test, not this.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T11:09:25-07:00",
          "tree_id": "262564dd5282d450d0b63069c74d4df502c4d637",
          "url": "https://github.com/nervosys/HyperMachine/commit/f3ff4a9dfb3e2656763f3aa029ee8ddf97755500"
        },
        "date": 1788459821589,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 557.703,
            "range": "+/- 0.523",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3771.397,
            "range": "+/- 45.346",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 440.699,
            "range": "+/- 1.065",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1165.546,
            "range": "+/- 28.556",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 407.324,
            "range": "+/- 1.993",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12922.701,
            "range": "+/- 76.954",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 652.111,
            "range": "+/- 1.826",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3881.117,
            "range": "+/- 15.318",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 515.054,
            "range": "+/- 2.059",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1224.102,
            "range": "+/- 3.939",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 481.324,
            "range": "+/- 3.701",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13846.519,
            "range": "+/- 102.041",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2898.511,
            "range": "+/- 28.019",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 112.886,
            "range": "+/- 1.904",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 134.443,
            "range": "+/- 1.813",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1232.95,
            "range": "+/- 2.394",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1972.936,
            "range": "+/- 5.389",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 703.497,
            "range": "+/- 1.722",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 875.758,
            "range": "+/- 1.319",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1061.319,
            "range": "+/- 1.052",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12032.656,
            "range": "+/- 8.288",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 499.948,
            "range": "+/- 0.993",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3259.317,
            "range": "+/- 2.01",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 359.539,
            "range": "+/- 0.347",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76068.534,
            "range": "+/- 351.656",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11739.908,
            "range": "+/- 185.425",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 405.539,
            "range": "+/- 1.607",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.455,
            "range": "+/- 0.313",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.225,
            "range": "+/- 0.286",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.169,
            "range": "+/- 0.201",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1633.637,
            "range": "+/- 5.897",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.992,
            "range": "+/- 0.215",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2013.477,
            "range": "+/- 13.472",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15810.62,
            "range": "+/- 163.559",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 865.188,
            "range": "+/- 0.819",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747638.605,
            "range": "+/- 390.389",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11796.329,
            "range": "+/- 7.295",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 278.58,
            "range": "+/- 0.486",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3057.142,
            "range": "+/- 4.916",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 137.003,
            "range": "+/- 0.373",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46834.474,
            "range": "+/- 36.496",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2525.914,
            "range": "+/- 8.896",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2236196.261,
            "range": "+/- 7192.93",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35395.836,
            "range": "+/- 140.429",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 890.58,
            "range": "+/- 2.405",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9178.544,
            "range": "+/- 40.929",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 326.392,
            "range": "+/- 0.758",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 139978.73,
            "range": "+/- 432.803",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6741.557,
            "range": "+/- 56.96",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7617.991,
            "range": "+/- 49.54",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "b85403aa02c19c3e66ee1472025f14de515a988e",
          "message": "feat(swarm): a command graph that decides who may talk to whom\n\nFirst piece of the orchestration layer. The substrate for it landed in\nfb6b5af, which took concurrent sandboxes from 23 to 1,000 at 0.157 MiB\neach; this decides what those thousand agents are allowed to say to each\nother.\n\nThe design question was where enforcement lives, and there was only one\ndefensible answer. If agents address each other directly, a permission\ngraph is advice. So `Swarm::send` is the only way a message moves, it\nconsults the graph before handing anything to a transport, and there is no\nsecond path. This repository already has the other arrangement --\n`AgentPolicy` records quotas and rate limits that nothing reads -- and an\nunconsulted rule is worse than an absent one, because it is believed.\n\nThe model is a command tree with explicit lateral edges:\n\n  down     an agent commands anything beneath it, at any depth\n  up       an agent reports to its immediate parent, and no further\n  sideways nothing, unless a grant says otherwise\n  else     refused, with the rule named\n\nThe asymmetry between the two vertical directions is deliberate. A\nsupervisor reaching a grandchild is delegation working; a grandchild\nreaching a grandparent is a subordinate choosing its own audience and\nrouting around every supervisor in between, so `SkipsLevel` names the\nparent it may address instead.\n\nGrants are one-way. A worker that may hand a finding to an auditor has not\nthereby agreed to take instructions back from it, and a symmetric grant\nwould quietly create the second edge.\n\nThe tests assert on delivery rather than on verdicts. A rule that returns\n\"denied\" while the message still arrives passes a verdict test and fails\nthe swarm, so `a_refused_message_does_not_arrive` checks the recipient's\ninbox is empty, and `revoking_a_grant_closes_the_edge` checks that exactly\nthe message sent while the grant was open is the one that got through.\n`a_thousand_agents_keep_their_boundaries` builds the shape this is for --\n1,001 agents, ten supervisors, ninety-nine workers each -- and checks the\nroot still commands a leaf nine hundred agents away while siblings cannot\nreach each other.\n\nTransport is a trait with one in-process implementation. That is a real\ndelivery mechanism rather than a stub, which is what makes the enforcement\ntests mean something. A vsock transport reaching an agent inside a\nunikernel is the next piece, and it changes where messages land, not who\nmay send them.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T12:10:10-07:00",
          "tree_id": "890768e7b71d706451df800eb858008a729b8218",
          "url": "https://github.com/nervosys/HyperMachine/commit/b85403aa02c19c3e66ee1472025f14de515a988e"
        },
        "date": 1788463631170,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 570.859,
            "range": "+/- 1.036",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3452.212,
            "range": "+/- 17.753",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 429.447,
            "range": "+/- 0.965",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1028.423,
            "range": "+/- 3.479",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 399.721,
            "range": "+/- 1.215",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 16563.143,
            "range": "+/- 53.896",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 629.094,
            "range": "+/- 1.416",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3823.116,
            "range": "+/- 8.694",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 497.787,
            "range": "+/- 2.083",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1204.396,
            "range": "+/- 4.43",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 456.784,
            "range": "+/- 3.356",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13257.148,
            "range": "+/- 46.082",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2710.043,
            "range": "+/- 5.696",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 102.146,
            "range": "+/- 0.228",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 119.514,
            "range": "+/- 0.609",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1224.395,
            "range": "+/- 6.673",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1962.11,
            "range": "+/- 7.801",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 653.716,
            "range": "+/- 2.151",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 849.849,
            "range": "+/- 4.148",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1104.292,
            "range": "+/- 7.889",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12030.097,
            "range": "+/- 11.86",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 504.672,
            "range": "+/- 1.559",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3280.778,
            "range": "+/- 5.97",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 358.414,
            "range": "+/- 0.451",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76407.532,
            "range": "+/- 360.042",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12108.425,
            "range": "+/- 229.951",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 436.313,
            "range": "+/- 10.828",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.464,
            "range": "+/- 0.263",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 102.737,
            "range": "+/- 0.229",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.802,
            "range": "+/- 0.209",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1577.833,
            "range": "+/- 3.603",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.627,
            "range": "+/- 0.204",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2061.863,
            "range": "+/- 14.767",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 19224.903,
            "range": "+/- 524.013",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.681,
            "range": "+/- 0.987",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747215.078,
            "range": "+/- 288.275",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11806.429,
            "range": "+/- 12.634",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.444,
            "range": "+/- 0.512",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3035.163,
            "range": "+/- 3.742",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.512,
            "range": "+/- 0.404",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46728.78,
            "range": "+/- 19.049",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2531.57,
            "range": "+/- 8.526",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2277034.174,
            "range": "+/- 13638.102",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35538.139,
            "range": "+/- 192.81",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 925.453,
            "range": "+/- 9.863",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9244.762,
            "range": "+/- 132.41",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 327.379,
            "range": "+/- 1.022",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 141949.997,
            "range": "+/- 841.715",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6773.4,
            "range": "+/- 28.544",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7632.136,
            "range": "+/- 57.45",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "committer": {
            "email": "opensource@nervosys.ai",
            "name": "nervosys",
            "username": "admercs"
          },
          "distinct": true,
          "id": "d93b78ee1ec9c4eaa478aa1663ba50246e7f554b",
          "message": "feat(swarm): capabilities, and a rule that stops delegation amplifying\n\nThe graph governed position and nothing else, so a grant could authorise a\nmessage the recipient had no ability to act on, and a supervisor could\ninstruct a subordinate to use an entitlement the supervisor did not hold.\nThe first turns a permission failure into a runtime one somewhere less\nconvenient; the second makes every capability in the swarm available to\nanyone with a subordinate who holds it.\n\nA message may now name a capability, and two rules apply when it does:\n\n  the recipient must hold it   -- or it cannot act on what it was sent\n  on a command, so must the    -- authority delegates downward but does\n  sender                          not amplify\n\nThe asymmetry is the point. The sender rule applies to commands only.\nReporting upward is not an exercise of the parent's authority, and\nrequiring a parent to hold whatever its child holds would make\nspecialisation impossible -- a supervisor coordinating a network worker\nand a disk worker would have to hold both entitlements to hear from\neither.\n\nPosition is checked before capability, so an agent that may not address\nsomeone at all learns that first and nothing about what that someone can\ndo.\n\n`Capability` is an opaque token rather than an enum. This crate decides\nwho may ask; what the names mean belongs to the caller, and hv2-agent's\n`CapabilitySet` maps onto these without this crate depending on it.\n\nSeven tests, including the one that matters: a supervisor commanding a\ncapability it does not hold is refused *and the message does not arrive*.\nThe thousand-agent example now exercises the same rule against real VMs:\n\n  no amplify : root -> w-988 refused; the worker holds 'net', the root does not\n  capability : root given 'net', same command now allowed\n\n18 tests pass, clippy silent with -D warnings.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T12:38:40-07:00",
          "tree_id": "076770bdf8ee38be556ad106dfebd1f1c4ecda6d",
          "url": "https://github.com/nervosys/HyperMachine/commit/d93b78ee1ec9c4eaa478aa1663ba50246e7f554b"
        },
        "date": 1788465273946,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 508.947,
            "range": "+/- 2.272",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8951.597,
            "range": "+/- 96.069",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 384.549,
            "range": "+/- 0.88",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1006.716,
            "range": "+/- 5.96",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 352.503,
            "range": "+/- 1.324",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13067.798,
            "range": "+/- 106.5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 586.081,
            "range": "+/- 8.178",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6255.602,
            "range": "+/- 24.988",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 440.458,
            "range": "+/- 1.782",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1118.786,
            "range": "+/- 3.889",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 420.492,
            "range": "+/- 4.997",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13073.309,
            "range": "+/- 70.285",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2451.205,
            "range": "+/- 18.211",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 110.719,
            "range": "+/- 2.327",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 243.915,
            "range": "+/- 1.048",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1116.191,
            "range": "+/- 6.298",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1824.877,
            "range": "+/- 13.527",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 590.62,
            "range": "+/- 0.846",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 765.003,
            "range": "+/- 3.928",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 990.505,
            "range": "+/- 8.321",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10663.485,
            "range": "+/- 17.52",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 458.02,
            "range": "+/- 1.727",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2902.217,
            "range": "+/- 4.68",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 326.451,
            "range": "+/- 0.839",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74382.369,
            "range": "+/- 764.374",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13047.075,
            "range": "+/- 159.106",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 363.004,
            "range": "+/- 0.474",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.356,
            "range": "+/- 0.239",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 94.568,
            "range": "+/- 0.145",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.453,
            "range": "+/- 0.272",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1414.588,
            "range": "+/- 4.125",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.489,
            "range": "+/- 0.217",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1938.289,
            "range": "+/- 12.818",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18094.238,
            "range": "+/- 455.968",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 735.575,
            "range": "+/- 0.923",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664025.587,
            "range": "+/- 522.374",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10471.792,
            "range": "+/- 24.977",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 250.369,
            "range": "+/- 0.722",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2691.329,
            "range": "+/- 1.612",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 131.117,
            "range": "+/- 0.761",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41420.924,
            "range": "+/- 29.437",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2250.455,
            "range": "+/- 6.889",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2017475.2,
            "range": "+/- 8471.405",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31543.17,
            "range": "+/- 162.491",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 809.425,
            "range": "+/- 5.843",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8131.231,
            "range": "+/- 63.338",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 305.231,
            "range": "+/- 2.731",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124620.269,
            "range": "+/- 420.744",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7586.296,
            "range": "+/- 93.554",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8389.089,
            "range": "+/- 90.136",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}