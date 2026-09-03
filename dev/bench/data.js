window.BENCHMARK_DATA = {
  "lastUpdate": 1788401470354,
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
      }
    ]
  }
}