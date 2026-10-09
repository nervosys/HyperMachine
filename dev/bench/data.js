window.BENCHMARK_DATA = {
  "lastUpdate": 1791568351467,
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
          "id": "aa24213766fa5310cb33aff7b90b5a843ff8cc4c",
          "message": "style(swarm): add the semicolon clippy wanted in the guest transport\n\n`block_in_place` closing without a trailing semicolon, which\nclippy::semicolon_if_nothing_returned rejects under -D warnings. The\nprevious commit was pushed before that lint ran.\n\nA follow-up rather than an amend: 739967b is already on master, and\nrewriting published history to hide a missing semicolon is a worse trade\nthan the extra commit.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T15:24:23-07:00",
          "tree_id": "29c0e707fe8d28f2e9b971b342f8f74ee956be36",
          "url": "https://github.com/nervosys/HyperMachine/commit/aa24213766fa5310cb33aff7b90b5a843ff8cc4c"
        },
        "date": 1788475208941,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 510.883,
            "range": "+/- 2.258",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8664.697,
            "range": "+/- 52.766",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 387.834,
            "range": "+/- 2.637",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1002.737,
            "range": "+/- 4.629",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 352.295,
            "range": "+/- 1.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13245.209,
            "range": "+/- 63.234",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 569.199,
            "range": "+/- 5.261",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4786.604,
            "range": "+/- 28.508",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 435.78,
            "range": "+/- 2.563",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1109.901,
            "range": "+/- 2.994",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 410.126,
            "range": "+/- 5.735",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12749.038,
            "range": "+/- 60.363",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2451.494,
            "range": "+/- 14.899",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 104.588,
            "range": "+/- 0.62",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.614,
            "range": "+/- 0.597",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1134.86,
            "range": "+/- 7.279",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1791.371,
            "range": "+/- 14.202",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 635.663,
            "range": "+/- 6.248",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 791.159,
            "range": "+/- 6.564",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 930.791,
            "range": "+/- 1.221",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10730.502,
            "range": "+/- 18.031",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 446.949,
            "range": "+/- 2.482",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2903.531,
            "range": "+/- 4.419",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 342.053,
            "range": "+/- 2.869",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72969.842,
            "range": "+/- 337.669",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13084.208,
            "range": "+/- 161.981",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 361.466,
            "range": "+/- 2.557",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.534,
            "range": "+/- 0.184",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.567,
            "range": "+/- 0.247",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.4,
            "range": "+/- 0.092",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1434.022,
            "range": "+/- 5.466",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.897,
            "range": "+/- 0.141",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1977.394,
            "range": "+/- 24.504",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17732.309,
            "range": "+/- 411.931",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.572,
            "range": "+/- 0.772",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 676738.465,
            "range": "+/- 6653.067",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10428.36,
            "range": "+/- 7.445",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.263,
            "range": "+/- 0.34",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2689.947,
            "range": "+/- 6.37",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 127.333,
            "range": "+/- 0.208",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41438.387,
            "range": "+/- 29.36",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2288.303,
            "range": "+/- 12.48",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2111982.88,
            "range": "+/- 20460.026",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31811.822,
            "range": "+/- 149.075",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 945.034,
            "range": "+/- 24.059",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8119.584,
            "range": "+/- 29.982",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 308.984,
            "range": "+/- 2.071",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 127750.673,
            "range": "+/- 639.067",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 8132.78,
            "range": "+/- 299.81",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8758.996,
            "range": "+/- 138.126",
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
          "id": "538fdb480252a8407e94b9903edb4ab2632730bc",
          "message": "docs: rewrite the handoff for where this actually stands\n\nThe previous handoff described two unmerged pull requests and a red\nmaster. Both are long gone: master is green, there are no open pull\nrequests and no open issues, and thirty-seven commits landed on 3\nSeptember.\n\nWhat the page leads with instead is the defect that outranks everything\nshipped: `VM::stop()` does not return, for any guest, and never has.\n`stop()` awaits the vCPU task handles, a task ends when `run_vcpu`\nreturns, and `KvmVcpu::run` retries `KVM_RUN` on EINTR unconditionally --\nwhich is the mechanism a VMM uses to kick a vCPU out. The vCPU is\nuninterruptible by construction, so every VM leaks its thread and no\nprocess using the API exits cleanly.\n\nThat last point reframes the rest of the page, and it says so: runs that\nappeared to finish were killed by their timeout after printing. The\nnumbers are unaffected because they were already on stdout, but claiming\nclean shutdowns would have been false.\n\nThe measurements are the other half. Cold start, memory per sandbox and\nconcurrency had never been measured here at all -- `vm_bench` times\nguest-memory allocation and snapshot serialisation, neither of which is a\nboot -- so no performance claim could be checked in either direction. They\nare now measured, against CubeSandbox's published figures, with the losses\nstated as plainly as the wins: 2.67ms unikernel cold start against their\n60ms, 0.157 MiB per sandbox against their 5 MB, 1,000 concurrent agents,\nand 1,014ms for a Linux guest, which is 17x slower than theirs and 97.5%\nguest kernel.\n\nThe four pillars are graded by whether they have been executed rather than\nwhether they compile, which leaves Type-1 honestly marked as a compilation\nresult.\n\nEnvironment notes are included because each of them cost time today: KVM\nneeds the user in the kvm group and `sudo` blocks on a password where\n`wsl -u root` does not; WSL builds need their own CARGO_TARGET_DIR or they\ninvalidate the Windows artifacts; protoc is missing in WSL so the full\nworkspace suite does not run there; WHP fails on this host; and paths\nthrough wsl.exe need MSYS_NO_PATHCONV=1.\n\nSame document, same design, same name. The head and stylesheet are\nuntouched.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-03T15:56:01-07:00",
          "tree_id": "c6e36217ef424e53e9ec7b344d9cc97567797f72",
          "url": "https://github.com/nervosys/HyperMachine/commit/538fdb480252a8407e94b9903edb4ab2632730bc"
        },
        "date": 1788477356005,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 548.844,
            "range": "+/- 1.448",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3422.888,
            "range": "+/- 24.82",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 410.335,
            "range": "+/- 1.853",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1018.29,
            "range": "+/- 4.368",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 371.473,
            "range": "+/- 1.157",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 21552.444,
            "range": "+/- 171.621",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 621.881,
            "range": "+/- 1.592",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3931.572,
            "range": "+/- 41.987",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 483.445,
            "range": "+/- 0.761",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1190.976,
            "range": "+/- 4.5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.867,
            "range": "+/- 1.609",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11827.638,
            "range": "+/- 30.714",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2715.466,
            "range": "+/- 8.84",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.266,
            "range": "+/- 0.433",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 119.795,
            "range": "+/- 0.38",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1199.278,
            "range": "+/- 5.704",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1935.005,
            "range": "+/- 6.245",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 643.736,
            "range": "+/- 1.131",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 837.423,
            "range": "+/- 3.52",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1070.299,
            "range": "+/- 2.336",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12072.903,
            "range": "+/- 16.535",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 498.803,
            "range": "+/- 1.095",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3303.208,
            "range": "+/- 8.138",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 358.505,
            "range": "+/- 0.477",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76366.983,
            "range": "+/- 347.604",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11022.07,
            "range": "+/- 36.136",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 412.134,
            "range": "+/- 0.638",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.619,
            "range": "+/- 0.087",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.027,
            "range": "+/- 0.257",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.871,
            "range": "+/- 0.152",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1612.204,
            "range": "+/- 6.685",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.471,
            "range": "+/- 0.342",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2086.121,
            "range": "+/- 7.21",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 25617.812,
            "range": "+/- 1031.704",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.716,
            "range": "+/- 0.977",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 783943.62,
            "range": "+/- 5533.945",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11803.127,
            "range": "+/- 12.733",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.617,
            "range": "+/- 0.494",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3047.371,
            "range": "+/- 9.63",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 143.956,
            "range": "+/- 1.075",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46839.964,
            "range": "+/- 39.58",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2764.747,
            "range": "+/- 62.055",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2230069.565,
            "range": "+/- 5877.9",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 36267.611,
            "range": "+/- 283.589",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 922.946,
            "range": "+/- 6.668",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9423.899,
            "range": "+/- 83.418",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 343.256,
            "range": "+/- 3.095",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140314.783,
            "range": "+/- 532.454",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6726.781,
            "range": "+/- 22.808",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7593.882,
            "range": "+/- 31.99",
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
          "id": "099ec308cdf062bdf70dd5f687dcf50fbd2937a9",
          "message": "docs: the thousand-agent figure now means a thousand agents\n\nThe number has been on this page for a while and it came from\n`unikernel_swarm`, whose agents halt immediately and never speak. That\nmeasures the hypervisor, not the swarm — a thousand VMs that execute one\ninstruction each is a real achievement about VMs and says nothing about\nagents. `vsock_swarm` had the agents and only three of them, because a\npolling guest cost a core.\n\nBoth halves are now the same run: 1,000 unikernels, each a full VM with\nits own vsock device, context ID and open connection, at 0.221 MiB and\n0.7% of one core across all of them while idle. The command graph's three\nchecks still pass against the first three.\n\nThe comparison table gains an idle-cost row, which is the figure this\nproject did not previously have and the one an agent fleet actually\nspends its time at. The concurrency row now says what the thousand\nconsists of, since \"1,000\" meant two different things on this page an\nhour ago.\n\nBoth caveats are on the page rather than in a commit message. The 37 ms\nper agent is serial setup and mostly polling granularity, not a boot\ntime — `rust_unikernel` puts a boot at 3.6 ms. And 0.221 MiB is more than\n`unikernel_swarm`'s 0.157 MiB because these agents carry a device, a\ndriver, an IDT and a connection.\n\n\"What to do next\" swaps the measurement for what it exposes: a thousand\nagents boot, connect, sleep and echo, which is plumbing proved end to end\nand is still not a workload.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T13:33:33-07:00",
          "tree_id": "5d5c8d823be7160361290af1d2d6c4305fe11797",
          "url": "https://github.com/nervosys/HyperMachine/commit/099ec308cdf062bdf70dd5f687dcf50fbd2937a9"
        },
        "date": 1788814310665,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 428.864,
            "range": "+/- 1.17",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2667.554,
            "range": "+/- 7.129",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 325.582,
            "range": "+/- 1.375",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 848.726,
            "range": "+/- 2.963",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 305.688,
            "range": "+/- 1.009",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11680.584,
            "range": "+/- 38.716",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 484.6,
            "range": "+/- 1.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3347.548,
            "range": "+/- 8.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 387.669,
            "range": "+/- 1.24",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 953.218,
            "range": "+/- 3.091",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 359.751,
            "range": "+/- 0.831",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12384.973,
            "range": "+/- 38.532",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2185.471,
            "range": "+/- 6.448",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 89.625,
            "range": "+/- 0.225",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 100.477,
            "range": "+/- 0.416",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1077.509,
            "range": "+/- 3.171",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1740.464,
            "range": "+/- 4.865",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 616.501,
            "range": "+/- 5.446",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 752.555,
            "range": "+/- 4.59",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 937.352,
            "range": "+/- 3.447",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11161.581,
            "range": "+/- 56.949",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 429.069,
            "range": "+/- 1.138",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3008.743,
            "range": "+/- 13.118",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 303.129,
            "range": "+/- 1.625",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 65884.35,
            "range": "+/- 752.885",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11030.676,
            "range": "+/- 44.399",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 381.662,
            "range": "+/- 1.204",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 14.869,
            "range": "+/- 0.12",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 98.438,
            "range": "+/- 0.526",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 18.644,
            "range": "+/- 0.058",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1560.519,
            "range": "+/- 2.971",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 30.96,
            "range": "+/- 0.423",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1649.235,
            "range": "+/- 9.346",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15924.73,
            "range": "+/- 79.562",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 753.962,
            "range": "+/- 2.199",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 687767.491,
            "range": "+/- 1006.23",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10919.935,
            "range": "+/- 25.792",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 243.82,
            "range": "+/- 0.706",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2767.263,
            "range": "+/- 9.191",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 113.83,
            "range": "+/- 0.363",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 43688.61,
            "range": "+/- 146.293",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1983.562,
            "range": "+/- 6.177",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1927606.434,
            "range": "+/- 19417.327",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27688.125,
            "range": "+/- 114.772",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 706.624,
            "range": "+/- 3.203",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7175.155,
            "range": "+/- 21.431",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 265.016,
            "range": "+/- 0.711",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 111900.312,
            "range": "+/- 883.963",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6847.007,
            "range": "+/- 19.044",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7686.93,
            "range": "+/- 26.718",
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
          "id": "5e4290ce04b740371d3637a2f0a5cee808dedfbc",
          "message": "docs: the density formula, measured, and what is left is the agent itself\n\nThe table on this page had \"what does an agent cost once it holds a\ncontext\" marked open, because weights being shared only settles half the\narithmetic. It is measured now: 24 MiB asked for and 24.184 MiB paid, an\noverhead of 0.184 MiB, which is what an agent costs holding nothing.\n\nSo the formula is the model once plus, per agent, exactly what that agent\ntouches — and it is on the page as a formula rather than a number,\nbecause the constant belongs to whoever picks the model. The rough sizing\nis there too: a small model's KV cache runs about 12 KiB per token, so a\n2,000-token context is around 24 MiB. That is stated as arithmetic to be\nchecked, not as something this project measured.\n\nThat was the last open question about the machine under an agent. The\nhypervisor's side of an agentic fleet is now measured end to end: a model\nshared once, a working set that costs what it is, a channel that reaches\ninside the guest, a policy that refuses, an idle agent that costs\nnothing. So the closing paragraph of that section changes from \"here is\nthe number to measure next\" to naming what has never run at all — a\nmodel, a tokeniser, a forward pass.\n\n\"What to do next\" leads with that, and says plainly that it is the first\nitem on the list whose difficulty is still a guess. Everything above it\non this page was measured before it was claimed; that one has not been\nattempted.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T14:37:53-07:00",
          "tree_id": "606a18478aea089f519ee8884209154cdb986e38",
          "url": "https://github.com/nervosys/HyperMachine/commit/5e4290ce04b740371d3637a2f0a5cee808dedfbc"
        },
        "date": 1788818188226,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 493.471,
            "range": "+/- 2.304",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3332.663,
            "range": "+/- 14.226",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 490.084,
            "range": "+/- 1.533",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 993.726,
            "range": "+/- 5.048",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 342.556,
            "range": "+/- 1.161",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 33202.121,
            "range": "+/- 225.314",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 579.622,
            "range": "+/- 2.32",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6808.386,
            "range": "+/- 43.365",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 446.097,
            "range": "+/- 2.904",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1099.214,
            "range": "+/- 3.865",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 406.02,
            "range": "+/- 1.396",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12958.205,
            "range": "+/- 51.919",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2396.143,
            "range": "+/- 8.732",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.63,
            "range": "+/- 0.323",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.24,
            "range": "+/- 0.319",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1111.843,
            "range": "+/- 6.187",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1817.756,
            "range": "+/- 8.664",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 592.387,
            "range": "+/- 4.527",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 750.21,
            "range": "+/- 1.597",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 939.572,
            "range": "+/- 2.614",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10645.851,
            "range": "+/- 9.807",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 452.295,
            "range": "+/- 2.189",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2980.285,
            "range": "+/- 15.564",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 318.243,
            "range": "+/- 0.48",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72591.228,
            "range": "+/- 347.966",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12620.451,
            "range": "+/- 77.528",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 358.564,
            "range": "+/- 2.276",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 20.409,
            "range": "+/- 0.367",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.342,
            "range": "+/- 0.556",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.184,
            "range": "+/- 0.233",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1384.864,
            "range": "+/- 4.976",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.685,
            "range": "+/- 0.451",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1924.305,
            "range": "+/- 8.195",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16983.712,
            "range": "+/- 134.822",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.335,
            "range": "+/- 0.613",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 661591.595,
            "range": "+/- 394.062",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10450.817,
            "range": "+/- 11.329",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.627,
            "range": "+/- 0.329",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2702.745,
            "range": "+/- 2.923",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 128.328,
            "range": "+/- 0.362",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41513.784,
            "range": "+/- 30.258",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2264.197,
            "range": "+/- 12.48",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2067626.4,
            "range": "+/- 18818.238",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 32167.695,
            "range": "+/- 177.456",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 799.837,
            "range": "+/- 4.093",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8518.881,
            "range": "+/- 111.284",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 298.651,
            "range": "+/- 1.534",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 126507.721,
            "range": "+/- 789.327",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7394.257,
            "range": "+/- 28.354",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8307.266,
            "range": "+/- 51.228",
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
          "id": "02608a0cb6defb4adf81fef9a730ac5b082bfda1",
          "message": "docs: bandwidth is what decides whether a fleet can use a model\n\nMemory decided whether a fleet could hold a model, and that was measured\ntwo commits ago. Bandwidth decides whether it can use one, and that is\nmeasured now: 1.66 cycles/byte warm in the guest, 13.20 cold, the\ndifference being one nested-paging fault per page paid once for the life\nof the VM.\n\nThe page keeps both numbers rather than averaging them, because a single\nfigure would have described a warm-up as a per-token cost — a factor of\neight in the wrong direction.\n\nWhat that rules out is on the page in as many words: a thousand agents\ninferring at once on a 350 MiB model would need hundreds of GB/s of\naggregate bandwidth, which no node has. A thousand mostly-idle agents is\na different proposition and is already measured at 0.7% of a core. So the\nfleet scales on the axis it was designed for — many cheap isolated\nsandboxes talking under a policy — and inference is a resource to be\nscheduled across them.\n\n\"What to do next\" is now a decision rather than a task, and is written as\none: inference scheduled in-guest keeps the isolation boundary around the\nmodel; inference as a capability the sandbox invokes keeps the guest as\nsmall as it is, which is the property a minimal sandbox exists for, and\nthe permission graph that would gate such a call already exists and\nalready refuses. The numbers are under it either way, and the choice is\nnot the measurement's to make.\n\nThe architecture table gains two rows, both answered: the guest can do\nthe arithmetic once SSE is enabled, and a fleet cannot infer\nconcurrently. It is worth noting the first needed `CR0.EM` cleared and\n`CR4.OSFXSR` set, and faulted silently until the fault reporter moved to\nthe first line of `kernel_main` — the same lesson this page already\ncarried, applied one layer too late.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T15:06:12-07:00",
          "tree_id": "ab49b6bbbe9c6b8d399c3d2a561d4af3b5d72c43",
          "url": "https://github.com/nervosys/HyperMachine/commit/02608a0cb6defb4adf81fef9a730ac5b082bfda1"
        },
        "date": 1788819698277,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 520.706,
            "range": "+/- 0.807",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3386.922,
            "range": "+/- 36.659",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 402.462,
            "range": "+/- 0.924",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1001.251,
            "range": "+/- 4.42",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 368.879,
            "range": "+/- 0.973",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12634.565,
            "range": "+/- 36.696",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 616.385,
            "range": "+/- 2.931",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3848.163,
            "range": "+/- 15.34",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 493.032,
            "range": "+/- 2.122",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1180.475,
            "range": "+/- 3.273",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 437.853,
            "range": "+/- 0.987",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12243.164,
            "range": "+/- 108.977",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2797.03,
            "range": "+/- 24.825",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 121.78,
            "range": "+/- 1.711",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 269.869,
            "range": "+/- 3.614",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1210.034,
            "range": "+/- 3.301",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1977.935,
            "range": "+/- 7.793",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 642.223,
            "range": "+/- 1.123",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 824.826,
            "range": "+/- 1.271",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1061.275,
            "range": "+/- 1.599",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12054.823,
            "range": "+/- 30.326",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 498.164,
            "range": "+/- 0.58",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3240.06,
            "range": "+/- 1.76",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 359.816,
            "range": "+/- 0.65",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77731.038,
            "range": "+/- 752.368",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12338.908,
            "range": "+/- 205.337",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 404.561,
            "range": "+/- 1.565",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.606,
            "range": "+/- 0.432",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 102.015,
            "range": "+/- 0.479",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.185,
            "range": "+/- 0.166",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1636.975,
            "range": "+/- 10.725",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.852,
            "range": "+/- 0.104",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2234.391,
            "range": "+/- 17.639",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16489.495,
            "range": "+/- 290.061",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.126,
            "range": "+/- 0.32",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747238.065,
            "range": "+/- 416.649",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11809.515,
            "range": "+/- 17.531",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.316,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3022.565,
            "range": "+/- 0.969",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.029,
            "range": "+/- 0.26",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46755.729,
            "range": "+/- 31.903",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2542.524,
            "range": "+/- 9.66",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2235997.652,
            "range": "+/- 8047.648",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35514.764,
            "range": "+/- 187.749",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 892.363,
            "range": "+/- 2.919",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9128.838,
            "range": "+/- 43.707",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 329.107,
            "range": "+/- 1.606",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 139544.421,
            "range": "+/- 331.654",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7047.534,
            "range": "+/- 93.316",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7803.422,
            "range": "+/- 61.411",
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
          "id": "56b32d7f271d9db4ec0302a3724b951b7bcf3968",
          "message": "docs: the guest has a heap, and .bss turns out not to be free\n\n\"Does the guest need a heap\" was marked open and almost certainly yes. It\nhas one: 64 KiB, so a tool request is bounded by memory rather than by a\n128-byte array that truncated silently — the wrong failure for a tool\ncall, since a truncated argument is a different call and not a refused\none.\n\nThe section is mostly about what it cost, because that was the\ninteresting part. `.bss` is not free here the way it is on an ordinary\nkernel: the Multiboot loader writes zeros across it, because guest RAM is\nonly reliably zero for a freshly created VM and a loader cannot assume it\nis being used that way. So a heap costs its full size per agent whether\nor not the agent allocates — 0.231 MiB each before, 0.302 after, at a\nhundred agents.\n\nAt a thousand that is 64 MiB of resident memory for an allocator most of\nthem will barely touch, which is the sort of cost that is invisible at\nthree agents and decides the shape of a fleet at a thousand. The page\nsays where the constant is and that changing it has a price.\n\nThat observation becomes the new item in \"What to do next\", because it is\na real saving sitting in plain sight: a fresh VM's memory *is* zero, and\nthe loader simply is not told which case it is in. Telling it would make\n`.bss` free until an agent touches it.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T15:43:43-07:00",
          "tree_id": "aac0f1c6e368350b994079152ac1dd5b34a807d9",
          "url": "https://github.com/nervosys/HyperMachine/commit/56b32d7f271d9db4ec0302a3724b951b7bcf3968"
        },
        "date": 1788822132013,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 538.497,
            "range": "+/- 6.092",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3622.327,
            "range": "+/- 49.854",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 405.884,
            "range": "+/- 2.708",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1078.381,
            "range": "+/- 11.558",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 359.045,
            "range": "+/- 2.86",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12746.648,
            "range": "+/- 84.449",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 570.425,
            "range": "+/- 1.875",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3661.627,
            "range": "+/- 15.977",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 455.31,
            "range": "+/- 4.526",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1115.517,
            "range": "+/- 4.855",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 459.109,
            "range": "+/- 7.678",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13146.207,
            "range": "+/- 84.972",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2416.603,
            "range": "+/- 6.641",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 116.376,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.596,
            "range": "+/- 0.834",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1102.262,
            "range": "+/- 3.212",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1858.415,
            "range": "+/- 20.223",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 602.751,
            "range": "+/- 3.675",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 760.465,
            "range": "+/- 1.929",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 927.281,
            "range": "+/- 0.752",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10616.16,
            "range": "+/- 7.457",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 437.814,
            "range": "+/- 0.693",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2881.693,
            "range": "+/- 4.694",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 318.729,
            "range": "+/- 0.83",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73626.979,
            "range": "+/- 712.209",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12567.829,
            "range": "+/- 42.047",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.876,
            "range": "+/- 1.138",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.331,
            "range": "+/- 0.083",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 99.633,
            "range": "+/- 0.343",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.513,
            "range": "+/- 0.122",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1415.369,
            "range": "+/- 4.919",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.655,
            "range": "+/- 0.264",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1975.155,
            "range": "+/- 6.604",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16645.434,
            "range": "+/- 101.115",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 735.861,
            "range": "+/- 1.535",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663054.601,
            "range": "+/- 419.969",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10435.922,
            "range": "+/- 11.999",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 248.614,
            "range": "+/- 0.685",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2684.235,
            "range": "+/- 2.732",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.984,
            "range": "+/- 0.377",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41537.112,
            "range": "+/- 104.599",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2259.078,
            "range": "+/- 12.908",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1988820.538,
            "range": "+/- 7263.885",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31358.094,
            "range": "+/- 110.941",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 800.628,
            "range": "+/- 4.427",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8100.155,
            "range": "+/- 33.244",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 301.591,
            "range": "+/- 2.889",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124692.19,
            "range": "+/- 471.649",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7429.323,
            "range": "+/- 34.956",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8237.277,
            "range": "+/- 30.175",
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
          "id": "db71d759f5f8b98486744a002c779c8d13374c19",
          "message": "docs: the heap's cost, and the fix that arrived an hour later\n\nThe previous version of this section reported a 64 KiB heap costing\n0.07 MiB per agent and said the constant was the number to change. That\nwas the wrong lever. The zeros the loader wrote across `.bss` were never\nwrong, only unnecessary — and the section now says which, because the\ndistinction is the interesting part: guest RAM is zero for a freshly\ncreated VM and is not zero for a snapshot restored into existing memory,\nand the loader had no way to tell those apart.\n\nSo it is told. `.bss` is carried as a range that must read as zero rather\nthan a block of zeros, and the backend is asked which case it is in,\ndefaulting to the conservative answer. The page keeps all three\nmeasurements in order — 0.231 before the heap, 0.302 with it, 0.242 now —\nbecause the shape of that sequence is the point.\n\nThe consequence is on the page too: a heap now costs the pages it\ntouches, so it could be considerably larger for very little. \"What to do\nnext\" changes from \"stop paying for .bss\" to deciding how big the heap\nshould be, which is a different question and a better one to be left\nwith.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T16:26:27-07:00",
          "tree_id": "4b2c2f0a8e16f217d50c5482d68b7b6dfaa81263",
          "url": "https://github.com/nervosys/HyperMachine/commit/db71d759f5f8b98486744a002c779c8d13374c19"
        },
        "date": 1788824510335,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 533.35,
            "range": "+/- 6.013",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3388.087,
            "range": "+/- 22.326",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 391.344,
            "range": "+/- 3.803",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1035.109,
            "range": "+/- 8.673",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 358.561,
            "range": "+/- 3.187",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12977.211,
            "range": "+/- 104.781",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 573.515,
            "range": "+/- 2.261",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6527.108,
            "range": "+/- 77.274",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 463.148,
            "range": "+/- 4.887",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1115.255,
            "range": "+/- 7.709",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 417.563,
            "range": "+/- 3.735",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13409.002,
            "range": "+/- 155.167",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2522.522,
            "range": "+/- 22.341",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 111.16,
            "range": "+/- 1.797",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 257.559,
            "range": "+/- 3.24",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1121.23,
            "range": "+/- 6.509",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1792.273,
            "range": "+/- 9.509",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 619.569,
            "range": "+/- 4.221",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 778.933,
            "range": "+/- 4.181",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 941.852,
            "range": "+/- 2.015",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10663.41,
            "range": "+/- 10.942",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 450.788,
            "range": "+/- 1.801",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2889.427,
            "range": "+/- 2.946",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 334.147,
            "range": "+/- 1.805",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72532.477,
            "range": "+/- 513.646",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12763.443,
            "range": "+/- 96.735",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 768.698,
            "range": "+/- 109.018",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.201,
            "range": "+/- 0.239",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 92.857,
            "range": "+/- 0.226",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.392,
            "range": "+/- 0.07",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1484.141,
            "range": "+/- 14.18",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.71,
            "range": "+/- 0.126",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1897.25,
            "range": "+/- 4.31",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17401.306,
            "range": "+/- 314.939",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 738.688,
            "range": "+/- 1.44",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 750616.6,
            "range": "+/- 8050.767",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10501.489,
            "range": "+/- 19.423",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 250.423,
            "range": "+/- 0.861",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2686.415,
            "range": "+/- 2.879",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 129.156,
            "range": "+/- 0.779",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41997.179,
            "range": "+/- 141.556",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2746.526,
            "range": "+/- 61.125",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2087575.2,
            "range": "+/- 21331.527",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33914.06,
            "range": "+/- 565.04",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 947.344,
            "range": "+/- 8.221",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8173.095,
            "range": "+/- 48.355",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 354.751,
            "range": "+/- 2.91",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 131703.67,
            "range": "+/- 1984.874",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7303.447,
            "range": "+/- 52.532",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8220.884,
            "range": "+/- 56.517",
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
          "id": "ef934a3f7bb93e48c0470eb14222f2d20d9adf2f",
          "message": "docs: messages now go both ways, and one of them looked like a leak\n\n\"Can agents talk to each other under a policy\" was already marked ran,\nand it was half true. Every message travelled into a guest; the guest\ncould only answer on its own connection, so agents interacting meant a\nhost relaying on their behalf. An agent that cannot address another agent\nis not interacting with anything.\n\nBoth directions now, and the row says so. The refusal is checked at the\nrecipient's console inside the recipient's VM, and the same message is\nsent again once the edge is granted — because a silent recipient and a\nbroken transport look identical without that second half.\n\nThe section keeps the false alarm, because it is the more useful half.\nThe first run appeared to show the graph delivering a message it had\nrefused. It had not: the example read the guest's replies with `peek`,\nwhich does not consume, so the second request arrived concatenated to the\nfirst and carried the refused text inside a payload that was permitted. A\ndemonstration that has to be right about a refusal cannot read its\nevidence with a function that keeps the evidence around.\n\n\"What to do next\" changes from sizing the heap — which is settled, 256\nKiB now that a heap costs the pages it touches — to giving the agent\nprotocol a shape. It is currently four prefixes parsed by hand with no\nframing, no request ids, and no way to have two things outstanding. That\nwas the right amount of protocol for proving each path exists and is the\nwrong amount for an agent holding a conversation while a tool call is\npending.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T16:44:20-07:00",
          "tree_id": "f43b5a7a1f6f4af722967a9f552e9dd782fa8e4b",
          "url": "https://github.com/nervosys/HyperMachine/commit/ef934a3f7bb93e48c0470eb14222f2d20d9adf2f"
        },
        "date": 1788825762418,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 357.863,
            "range": "+/- 5.635",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2318.752,
            "range": "+/- 29.692",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 247.671,
            "range": "+/- 0.695",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 650.062,
            "range": "+/- 4.17",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 236.921,
            "range": "+/- 0.694",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11967.807,
            "range": "+/- 113.043",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 376.946,
            "range": "+/- 0.912",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2478.882,
            "range": "+/- 8.408",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 298.718,
            "range": "+/- 0.65",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 749.1,
            "range": "+/- 2.603",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 278.332,
            "range": "+/- 0.723",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 14389.358,
            "range": "+/- 33.241",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 1723.568,
            "range": "+/- 14.088",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 66.568,
            "range": "+/- 0.22",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 79.092,
            "range": "+/- 0.948",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 835.216,
            "range": "+/- 3.646",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1362.752,
            "range": "+/- 9.01",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 444.187,
            "range": "+/- 2.087",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 588.634,
            "range": "+/- 5.171",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 722.105,
            "range": "+/- 1.566",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8716.289,
            "range": "+/- 56.649",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 332.528,
            "range": "+/- 0.563",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2314.079,
            "range": "+/- 14.113",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 236.773,
            "range": "+/- 0.666",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 49932.517,
            "range": "+/- 405.381",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8047.632,
            "range": "+/- 17.84",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 296.734,
            "range": "+/- 1.015",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 11.82,
            "range": "+/- 0.118",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 75.478,
            "range": "+/- 0.138",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 14.672,
            "range": "+/- 0.092",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1203.496,
            "range": "+/- 2.576",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 23.059,
            "range": "+/- 0.093",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1201.465,
            "range": "+/- 4.895",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11127.351,
            "range": "+/- 40.745",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 626.908,
            "range": "+/- 7.508",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 586072.429,
            "range": "+/- 2195.248",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8216.985,
            "range": "+/- 20.101",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 181.429,
            "range": "+/- 0.39",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2155.936,
            "range": "+/- 12.698",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 90.297,
            "range": "+/- 0.642",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 32610.628,
            "range": "+/- 53.57",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1569.067,
            "range": "+/- 10.109",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1386879.923,
            "range": "+/- 8456.701",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 21233.974,
            "range": "+/- 67.329",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 560.972,
            "range": "+/- 4.064",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 5405.912,
            "range": "+/- 13.806",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 214.646,
            "range": "+/- 1.383",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 86627.46,
            "range": "+/- 583.699",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5004.338,
            "range": "+/- 33.159",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5347.32,
            "range": "+/- 13.09",
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
          "id": "516a7c804671e82b75303ac3386af9a0f85a70f4",
          "message": "docs: rewrite the handoff as a document rather than an accretion\n\nIt had been appended to after every commit for a week, which showed: the\nmasthead named a commit five ahead of it, sections sat in the order they\nhappened rather than the order they are read in, and the largest gap on the\npage — that no model has ever run inside any of this — was mentioned in\npassing three times and stated nowhere.\n\nRewritten around four questions a reader actually arrives with:\n\n- What is the machine underneath, and what was wrong with it\n- Is there a guest worth putting an agent in\n- What does a fleet of them cost\n- What turns one into an agent\n\nEvery measurement is carried over unchanged; nothing was re-stated more\nfavourably. What is new is orientation:\n\n- \"How to read this page\" says what ran / typed / none mean up front, and\n  on what host.\n- \"Run any of it in one command\" is a table of the twelve examples, the\n  crate each lives in, and what each settles. There was no way in before\n  except reading the tree.\n- \"No model has ever run inside one of these guests\" is now the first open\n  item and a row in the comparison table, rather than a caveat at the end of\n  paragraphs. It is the largest thing missing and it should read that way.\n- The masthead, the facts and the footer say where this actually stands,\n  including that a model has never run.\n\nStructure verified balanced: 13 sections, 5 tables, 99 paragraphs, all tags\nclosed, one style block, no doctype/html/body tags.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-07T17:25:41-07:00",
          "tree_id": "260e7f63e5b35089470b1d72da2abe8c37525166",
          "url": "https://github.com/nervosys/HyperMachine/commit/516a7c804671e82b75303ac3386af9a0f85a70f4"
        },
        "date": 1788828256761,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 514.576,
            "range": "+/- 3.119",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3602.174,
            "range": "+/- 44.189",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 389.236,
            "range": "+/- 1.93",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1022.794,
            "range": "+/- 4.557",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 349.479,
            "range": "+/- 2.122",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 37390.094,
            "range": "+/- 663.173",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 571.152,
            "range": "+/- 2.571",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3663.278,
            "range": "+/- 27.814",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 437.551,
            "range": "+/- 1.055",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1106.967,
            "range": "+/- 5.379",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 398.069,
            "range": "+/- 2.004",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12822.781,
            "range": "+/- 37.008",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2436.437,
            "range": "+/- 5.335",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.996,
            "range": "+/- 0.673",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 128.916,
            "range": "+/- 3.471",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1101.504,
            "range": "+/- 3.741",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1763.859,
            "range": "+/- 3.43",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 579.014,
            "range": "+/- 1.22",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 747.428,
            "range": "+/- 1.238",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 937.793,
            "range": "+/- 1.98",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10626.377,
            "range": "+/- 12.992",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 442.384,
            "range": "+/- 1.824",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2871.333,
            "range": "+/- 1.284",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 320.544,
            "range": "+/- 0.861",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77455.725,
            "range": "+/- 391.062",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12824.187,
            "range": "+/- 92.89",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 362.048,
            "range": "+/- 1.739",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.184,
            "range": "+/- 0.072",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.052,
            "range": "+/- 0.281",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.52,
            "range": "+/- 0.113",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1411.012,
            "range": "+/- 2.152",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.732,
            "range": "+/- 0.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1994.574,
            "range": "+/- 24.639",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16608.911,
            "range": "+/- 91.889",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 741.26,
            "range": "+/- 1.07",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 666972.529,
            "range": "+/- 1210.209",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10656.286,
            "range": "+/- 47.555",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.635,
            "range": "+/- 0.411",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2717.797,
            "range": "+/- 5.448",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 132.877,
            "range": "+/- 0.872",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41788.511,
            "range": "+/- 62.947",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2269.027,
            "range": "+/- 10.655",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1999823.504,
            "range": "+/- 8598.832",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31326.271,
            "range": "+/- 107.093",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 801.246,
            "range": "+/- 3.389",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8232.096,
            "range": "+/- 63.966",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 331.819,
            "range": "+/- 7.161",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125146.384,
            "range": "+/- 739.318",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7637.907,
            "range": "+/- 93.37",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8252.89,
            "range": "+/- 22.842",
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
          "id": "81bc8831a558b0c26e27bcca88fc87772d061ce2",
          "message": "deps: IronCrypto from crates.io, and the private-repo plumbing goes with it\n\nThe `ic-*` crates were published on 2026-09-22, so the pinned git rev has\nnothing left to do. That pin existed because the repository was private, not\nbecause a particular revision was wanted, and a version requirement is the\nright shape for a dependency you can name.\n\nAll ten git dependencies -- seven in hv2-core, three in hv2-api -- are now\n`\"0.1.1\"` from the registry, and every `ic-*` entry in Cargo.lock moved from\n`git+https://...?rev=ad762973` to `registry+https://github.com/rust-lang/\ncrates.io-index`.\n\nVerified before anything else was touched, because a git-to-registry move is\nnot a no-op and 0.1.1 is not the rev we built against. The published vectors\npass: SP 800-38D AES-256-GCM on ciphertext *and* tag, RFC 4231 HMAC, RFC 5869\nHKDF, FIPS 180-4 SHA-2, plus P-521 key generation and AEAD tamper rejection.\n36 of 36. That is evidence of identical bytes rather than a compatible API,\nwhich was the one thing worth refusing to assume. Licences are unchanged too:\nall seven publish as AGPL-3.0-or-later, which deny.toml already allows.\n\nThen the machinery that existed only to reach a private repository:\n\n  * 29 credential steps across six workflows\n  * `.github/actions/private-git-deps` itself\n  * bench.yml's inline auth step -- the one that could not use the composite\n    action because both its checkouts set `path:`, so nothing landed at the\n    workspace root for a local action to be found at\n  * the Containerfile's BuildKit secret mount, the `git config` dance it\n    guarded, and the `# syntax` directive that `--mount=type=secret` needed\n  * deploy.yml's `secrets:` block\n  * `[net] git-fetch-with-cli` from .cargo/config.toml\n\n`IRONCRYPTO_TOKEN` is now required by nothing. CI, a container build and a\nlaptop all fetch this workspace without a credential.\n\nAlso here, from a pre-publication scan of the 200 unpushed commits:\n`media/publish_remaining.ps1` hardcoded `C:\\Users\\adamm\\dev\\nervosys\\os\\\nAetherVM\\...`, which published one laptop's directory layout and had stopped\nresolving anywhere, the project having been renamed. Now `$PSScriptRoot`.\nThe scan found nothing else -- no tokens, keys or private-key blocks across\nthe 3 MB diff, two distinct email addresses (an Anthropic no-reply and the\norg's public contact), and the 167 MB of session transcripts under\ndocs/sessions/ are gitignored with none tracked.\n\nSweep: fmt clean, clippy 0 with exit 0 for workspace and hv1-multiboot,\nrustdoc 0, 5461 tests across 92 result lines, 46 examples.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-22T23:45:59-07:00",
          "tree_id": "958ff307ad233ece274e57a89e10a48e63dd6aa2",
          "url": "https://github.com/nervosys/HyperMachine/commit/81bc8831a558b0c26e27bcca88fc87772d061ce2"
        },
        "date": 1790147445101,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4571.434,
            "range": "+/- 10.776",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 15938.182,
            "range": "+/- 63.356",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3995.937,
            "range": "+/- 7.416",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6778.037,
            "range": "+/- 11.089",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3911.11,
            "range": "+/- 16.6",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 51977.877,
            "range": "+/- 87.819",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4701.015,
            "range": "+/- 13.496",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 18214.893,
            "range": "+/- 83.553",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4131.308,
            "range": "+/- 8.715",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 7271.798,
            "range": "+/- 113.247",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3997.092,
            "range": "+/- 8.873",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 54260.244,
            "range": "+/- 460.933",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 19182.938,
            "range": "+/- 51.368",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 102.557,
            "range": "+/- 1.165",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.64,
            "range": "+/- 0.611",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4690.39,
            "range": "+/- 55.896",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 8629.968,
            "range": "+/- 104.691",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1874.897,
            "range": "+/- 20.11",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2913.847,
            "range": "+/- 41.785",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1258.162,
            "range": "+/- 3.613",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10947.282,
            "range": "+/- 19.994",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 769.364,
            "range": "+/- 3.78",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3193.154,
            "range": "+/- 3.925",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 649.537,
            "range": "+/- 3.345",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74113.437,
            "range": "+/- 626.997",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12644.378,
            "range": "+/- 103.857",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 353.925,
            "range": "+/- 1.113",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.24,
            "range": "+/- 0.238",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 96.996,
            "range": "+/- 0.356",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.944,
            "range": "+/- 0.113",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1539.35,
            "range": "+/- 49.573",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.623,
            "range": "+/- 0.121",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1926.192,
            "range": "+/- 5.51",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16567.744,
            "range": "+/- 79.066",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 743.843,
            "range": "+/- 0.816",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662781.931,
            "range": "+/- 1373.423",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10461.264,
            "range": "+/- 7.087",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 260.044,
            "range": "+/- 0.267",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2684.085,
            "range": "+/- 5.287",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.504,
            "range": "+/- 0.258",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41464.381,
            "range": "+/- 34.975",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 3133.232,
            "range": "+/- 5.496",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2852715.263,
            "range": "+/- 23305.304",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 43545.393,
            "range": "+/- 102.926",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 1119.388,
            "range": "+/- 8.33",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 11231.778,
            "range": "+/- 30.681",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 444.285,
            "range": "+/- 1.811",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 172774.628,
            "range": "+/- 461.227",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7412.203,
            "range": "+/- 70.459",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8734.026,
            "range": "+/- 104.769",
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
          "id": "bbc7c61e067b23ddaf2f01a979687387c9b39b53",
          "message": "docs(readme): the banner replaces the title heading\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv",
          "timestamp": "2026-09-26T22:44:51-07:00",
          "tree_id": "8aa744061ad021324fde2296c1fbd7dc5ec64166",
          "url": "https://github.com/nervosys/HyperMachine/commit/bbc7c61e067b23ddaf2f01a979687387c9b39b53"
        },
        "date": 1790489007651,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4566.322,
            "range": "+/- 17.088",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 15931.816,
            "range": "+/- 65.117",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4007.78,
            "range": "+/- 9.689",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6816.205,
            "range": "+/- 21.295",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3884.578,
            "range": "+/- 9.286",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 78110.476,
            "range": "+/- 250.879",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4751.092,
            "range": "+/- 20.538",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 19373.035,
            "range": "+/- 100.67",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4144.408,
            "range": "+/- 10.384",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 7026.788,
            "range": "+/- 24.607",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4007.352,
            "range": "+/- 8.978",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 54535.948,
            "range": "+/- 209.308",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 19663.452,
            "range": "+/- 182.531",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.183,
            "range": "+/- 0.623",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.031,
            "range": "+/- 0.875",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4757.482,
            "range": "+/- 49.807",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 8698.775,
            "range": "+/- 97.704",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1923.642,
            "range": "+/- 34.881",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2703.407,
            "range": "+/- 13.075",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1247.957,
            "range": "+/- 1.605",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11032.588,
            "range": "+/- 16.755",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 762.046,
            "range": "+/- 1.551",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3201.791,
            "range": "+/- 10.509",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 649.965,
            "range": "+/- 4.778",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78822.064,
            "range": "+/- 1031.454",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13182.571,
            "range": "+/- 154.65",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 379.798,
            "range": "+/- 7.408",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.745,
            "range": "+/- 0.3",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 91.354,
            "range": "+/- 0.341",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.95,
            "range": "+/- 0.322",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1410.169,
            "range": "+/- 5.608",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.695,
            "range": "+/- 0.262",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1923.844,
            "range": "+/- 5.421",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17558.567,
            "range": "+/- 288.5",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 742.953,
            "range": "+/- 0.411",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662737.268,
            "range": "+/- 409.453",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10427.251,
            "range": "+/- 7.241",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 259.239,
            "range": "+/- 0.288",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2683.705,
            "range": "+/- 4.41",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.136,
            "range": "+/- 0.312",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41379.537,
            "range": "+/- 20.871",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 3139.975,
            "range": "+/- 8.721",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2799668.389,
            "range": "+/- 11257.933",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 43673.042,
            "range": "+/- 197.713",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 1110.907,
            "range": "+/- 3.31",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 11230.111,
            "range": "+/- 40.141",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 445.568,
            "range": "+/- 1.257",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 176041.348,
            "range": "+/- 1070.741",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7367.103,
            "range": "+/- 38.361",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 9187.067,
            "range": "+/- 195.367",
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
          "id": "c4c6d0c44719e3c7dc2d77c7621ea14b09752da7",
          "message": "Merge pull request #104 from nervosys/docs/sandbox-refusal-plumbing\n\ndocs(sandbox): tell integrators their error path can eat the refusal",
          "timestamp": "2026-09-27T10:47:48-07:00",
          "tree_id": "7b819a8abfd213ada0ef12f3dabeefdbf808b595",
          "url": "https://github.com/nervosys/HyperMachine/commit/c4c6d0c44719e3c7dc2d77c7621ea14b09752da7"
        },
        "date": 1790532324599,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 5062.644,
            "range": "+/- 14.014",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 17629.53,
            "range": "+/- 77.039",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4438.086,
            "range": "+/- 9.005",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 7520.166,
            "range": "+/- 18.918",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4303.948,
            "range": "+/- 8.416",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 57709.439,
            "range": "+/- 171.557",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5217.183,
            "range": "+/- 7.869",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 18296.983,
            "range": "+/- 40.394",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4641.552,
            "range": "+/- 23.866",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 7732.215,
            "range": "+/- 10.047",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4453.995,
            "range": "+/- 11.061",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 59683.829,
            "range": "+/- 160.565",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 21300.292,
            "range": "+/- 62.748",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.917,
            "range": "+/- 0.321",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.806,
            "range": "+/- 0.356",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4830.598,
            "range": "+/- 13.397",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 8597.291,
            "range": "+/- 19.276",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1970.266,
            "range": "+/- 4.858",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2934.412,
            "range": "+/- 11.413",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1410.987,
            "range": "+/- 1.944",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12380.26,
            "range": "+/- 17.342",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 862.368,
            "range": "+/- 1.623",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3607.436,
            "range": "+/- 4.465",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 723.634,
            "range": "+/- 1.235",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75948.024,
            "range": "+/- 473.086",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11383.506,
            "range": "+/- 75.877",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 435.496,
            "range": "+/- 1.124",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.109,
            "range": "+/- 0.06",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 109.789,
            "range": "+/- 0.234",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.915,
            "range": "+/- 0.027",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1736.239,
            "range": "+/- 3.101",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.717,
            "range": "+/- 0.171",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2002.783,
            "range": "+/- 6.198",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15380.731,
            "range": "+/- 82.616",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.817,
            "range": "+/- 1.053",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 748573.823,
            "range": "+/- 240.369",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11796.227,
            "range": "+/- 3.521",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 290.778,
            "range": "+/- 0.409",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3031.756,
            "range": "+/- 1.641",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 153.897,
            "range": "+/- 0.242",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46854.918,
            "range": "+/- 23.664",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2428.499,
            "range": "+/- 7.523",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2119054.417,
            "range": "+/- 5765.792",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33677.484,
            "range": "+/- 193.953",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 875.313,
            "range": "+/- 1.843",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8616.534,
            "range": "+/- 18.34",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 379.105,
            "range": "+/- 1.277",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 132579.326,
            "range": "+/- 437.739",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6842.511,
            "range": "+/- 39.822",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7635.675,
            "range": "+/- 58.297",
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
          "id": "097e48d27f7e75f122ba2097e53f8adfa9a007c3",
          "message": "Merge pull request #89 from nervosys/dependabot/github_actions/slackapi/slack-github-action-4\n\nci: bump slackapi/slack-github-action from 1 to 4",
          "timestamp": "2026-09-28T07:06:19-07:00",
          "tree_id": "4b326bc86ff01d0ce85ad5b53707ab7db2f49aab",
          "url": "https://github.com/nervosys/HyperMachine/commit/097e48d27f7e75f122ba2097e53f8adfa9a007c3"
        },
        "date": 1790605712508,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3968.818,
            "range": "+/- 14.288",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 13925.632,
            "range": "+/- 103.092",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3516.623,
            "range": "+/- 24.908",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6032.522,
            "range": "+/- 34.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3329.179,
            "range": "+/- 3.2",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 44958.428,
            "range": "+/- 114.488",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4074.229,
            "range": "+/- 8.365",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 14189.841,
            "range": "+/- 23.674",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3582.97,
            "range": "+/- 6.204",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6045.608,
            "range": "+/- 14.325",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3467.705,
            "range": "+/- 10.449",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 46181.022,
            "range": "+/- 128.839",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 16545.511,
            "range": "+/- 61.179",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 82.951,
            "range": "+/- 0.393",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 105.215,
            "range": "+/- 0.3",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 3728.281,
            "range": "+/- 4.379",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 6677.842,
            "range": "+/- 12.766",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1538.791,
            "range": "+/- 4.88",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2285.181,
            "range": "+/- 7.621",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1076.411,
            "range": "+/- 1.19",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9594.232,
            "range": "+/- 14.46",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 653.6,
            "range": "+/- 1.092",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2782.312,
            "range": "+/- 4.237",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 549.04,
            "range": "+/- 1.255",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59573.449,
            "range": "+/- 377.031",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8793.641,
            "range": "+/- 78.756",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 313.758,
            "range": "+/- 1.479",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.47,
            "range": "+/- 0.107",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 82.982,
            "range": "+/- 0.163",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.709,
            "range": "+/- 0.172",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1355.516,
            "range": "+/- 22.056",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 27.636,
            "range": "+/- 0.252",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1549.983,
            "range": "+/- 2.97",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12504.519,
            "range": "+/- 135.614",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 656.746,
            "range": "+/- 0.966",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 580345.594,
            "range": "+/- 235.775",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9172.396,
            "range": "+/- 8.082",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 230.718,
            "range": "+/- 0.809",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2365.226,
            "range": "+/- 3.708",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 120.532,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 37108.504,
            "range": "+/- 381.424",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1873.513,
            "range": "+/- 3.238",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1643477.649,
            "range": "+/- 4770.751",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 26849.492,
            "range": "+/- 235.556",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 681.066,
            "range": "+/- 2.635",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6723.603,
            "range": "+/- 18.892",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 295.954,
            "range": "+/- 1.306",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 102429.274,
            "range": "+/- 237.759",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5370.626,
            "range": "+/- 29.047",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 6219.007,
            "range": "+/- 81.647",
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
          "id": "16c1464e4698fc6b3c4cb5b0846132da64587ff1",
          "message": "Merge pull request #108 from nervosys/ci/pin-actions\n\nci: pin every action to a commit SHA",
          "timestamp": "2026-09-28T08:10:04-07:00",
          "tree_id": "cb809e730f7118e541a7b5e95cd362ec2e611100",
          "url": "https://github.com/nervosys/HyperMachine/commit/16c1464e4698fc6b3c4cb5b0846132da64587ff1"
        },
        "date": 1790609193047,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 2942.818,
            "range": "+/- 3.367",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 10725.967,
            "range": "+/- 28.956",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 2562.677,
            "range": "+/- 3.108",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4453.175,
            "range": "+/- 11.934",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2486.452,
            "range": "+/- 13.633",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 36082.63,
            "range": "+/- 96.219",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 3081.196,
            "range": "+/- 19.933",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11664.832,
            "range": "+/- 92.227",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 2609.591,
            "range": "+/- 6.75",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4711.644,
            "range": "+/- 9.241",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 2627.531,
            "range": "+/- 8.168",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 40652.968,
            "range": "+/- 637.474",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 13685.143,
            "range": "+/- 48.841",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 71.107,
            "range": "+/- 0.707",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 85.956,
            "range": "+/- 0.714",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 3129.808,
            "range": "+/- 11.923",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 5663.632,
            "range": "+/- 28.266",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1210.748,
            "range": "+/- 4.489",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 1878.764,
            "range": "+/- 2.393",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 894.962,
            "range": "+/- 2.668",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8233.049,
            "range": "+/- 81.923",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 531.063,
            "range": "+/- 2.53",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2310.196,
            "range": "+/- 4.755",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 432.074,
            "range": "+/- 0.985",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 45017.813,
            "range": "+/- 56.757",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 6403.265,
            "range": "+/- 48.48",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 404.575,
            "range": "+/- 0.983",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 12.465,
            "range": "+/- 0.033",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 100.975,
            "range": "+/- 0.207",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.489,
            "range": "+/- 0.018",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1615.606,
            "range": "+/- 3.821",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 28.791,
            "range": "+/- 0.091",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1162.267,
            "range": "+/- 1.256",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 8165.803,
            "range": "+/- 56.825",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 549.011,
            "range": "+/- 1.575",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 482360.186,
            "range": "+/- 773.759",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 7602.25,
            "range": "+/- 14.255",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 187.601,
            "range": "+/- 0.862",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 1958.622,
            "range": "+/- 4.24",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 93.447,
            "range": "+/- 0.229",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 30628.699,
            "range": "+/- 88.565",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1404.426,
            "range": "+/- 2.367",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1164271.547,
            "range": "+/- 4727.132",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 18957.751,
            "range": "+/- 55.572",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 509.043,
            "range": "+/- 1.971",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 4911.738,
            "range": "+/- 41.78",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 224.643,
            "range": "+/- 0.62",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 72388.889,
            "range": "+/- 164.078",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 3843.2,
            "range": "+/- 15.662",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 4470.739,
            "range": "+/- 43.462",
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
          "id": "8f4fb5bb5089c908a7df9f91aa5896dda3dfb73a",
          "message": "Merge pull request #91 from nervosys/dependabot/cargo/base64-0.23.1\n\ndeps(deps): Bump base64 from 0.22.1 to 0.23.1",
          "timestamp": "2026-09-28T08:57:15-07:00",
          "tree_id": "0482af0b22eeb0e5e8765cf3e7307496a6cbe505",
          "url": "https://github.com/nervosys/HyperMachine/commit/8f4fb5bb5089c908a7df9f91aa5896dda3dfb73a"
        },
        "date": 1790612180059,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4621.335,
            "range": "+/- 39.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 16167.744,
            "range": "+/- 140.29",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3977.595,
            "range": "+/- 35.812",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6564.577,
            "range": "+/- 55.102",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3836.372,
            "range": "+/- 36.24",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 51137.148,
            "range": "+/- 568.535",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4793.221,
            "range": "+/- 53.708",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 16293.229,
            "range": "+/- 133.342",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4169.105,
            "range": "+/- 40.003",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6764.894,
            "range": "+/- 63.306",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3949.062,
            "range": "+/- 27.764",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 51748.98,
            "range": "+/- 416.939",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 18944.813,
            "range": "+/- 177.614",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 92.692,
            "range": "+/- 0.801",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 104.653,
            "range": "+/- 1.02",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4394.612,
            "range": "+/- 44.138",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 7889.172,
            "range": "+/- 67.564",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1785.197,
            "range": "+/- 14.257",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2680.038,
            "range": "+/- 28.089",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1274.789,
            "range": "+/- 12.99",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11325.016,
            "range": "+/- 125.283",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 792.607,
            "range": "+/- 6.378",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3357.166,
            "range": "+/- 30.798",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 667.793,
            "range": "+/- 6.813",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 66161.768,
            "range": "+/- 688.251",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 10401.128,
            "range": "+/- 98.616",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 345.234,
            "range": "+/- 2.856",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 15.211,
            "range": "+/- 0.153",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 89.363,
            "range": "+/- 0.729",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 19.461,
            "range": "+/- 0.178",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1361.972,
            "range": "+/- 11.936",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 30.565,
            "range": "+/- 0.311",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1609.387,
            "range": "+/- 11.856",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11877.202,
            "range": "+/- 40.742",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 755.632,
            "range": "+/- 5.676",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 692184.87,
            "range": "+/- 5503.797",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10899.576,
            "range": "+/- 91.43",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 260.757,
            "range": "+/- 2.222",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2817.352,
            "range": "+/- 21.43",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.364,
            "range": "+/- 0.947",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 44094.936,
            "range": "+/- 421.232",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2145.407,
            "range": "+/- 20.801",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2022748.176,
            "range": "+/- 24525.852",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30264.936,
            "range": "+/- 297.106",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 840.906,
            "range": "+/- 14.132",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7802.266,
            "range": "+/- 73.047",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 342.204,
            "range": "+/- 3.099",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 119650.209,
            "range": "+/- 1138.153",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5456.979,
            "range": "+/- 50.047",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5923.612,
            "range": "+/- 31.332",
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
          "id": "f434743de2ab825c7ed0a4b1de3214a61a86a347",
          "message": "Merge pull request #109 from nervosys/ci/missing-secrets-visible\n\nci: say so when Slack or Codecov is skipped for want of a secret",
          "timestamp": "2026-09-28T12:18:32-07:00",
          "tree_id": "e6f226dec52c0694ad464cd18e879ec0532b1738",
          "url": "https://github.com/nervosys/HyperMachine/commit/f434743de2ab825c7ed0a4b1de3214a61a86a347"
        },
        "date": 1790624065032,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3921.697,
            "range": "+/- 8.668",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 14400.862,
            "range": "+/- 133.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3443.048,
            "range": "+/- 7.311",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5917.711,
            "range": "+/- 23.192",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3391.989,
            "range": "+/- 22.138",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 47919.114,
            "range": "+/- 491.797",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4070.6,
            "range": "+/- 11.514",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 14330.729,
            "range": "+/- 58.331",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3559.811,
            "range": "+/- 5.081",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6035.971,
            "range": "+/- 21.452",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3448.489,
            "range": "+/- 8.079",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 51437.072,
            "range": "+/- 348.75",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 16445.437,
            "range": "+/- 43.95",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 77.239,
            "range": "+/- 0.185",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 91.58,
            "range": "+/- 0.478",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 3776.647,
            "range": "+/- 17.203",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 6671.603,
            "range": "+/- 15.434",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1541.2,
            "range": "+/- 3.85",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2270.205,
            "range": "+/- 5.021",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1077.931,
            "range": "+/- 2.041",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9594.41,
            "range": "+/- 8.675",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 654.855,
            "range": "+/- 1.473",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2818.046,
            "range": "+/- 9.445",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 549.12,
            "range": "+/- 1.58",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59023.207,
            "range": "+/- 276.218",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8753.097,
            "range": "+/- 57.642",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 304.534,
            "range": "+/- 0.719",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 12.643,
            "range": "+/- 0.019",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 77.567,
            "range": "+/- 0.051",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 16.902,
            "range": "+/- 0.068",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1209.558,
            "range": "+/- 1.848",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 26.326,
            "range": "+/- 0.035",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1631.488,
            "range": "+/- 14.145",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11870.821,
            "range": "+/- 47.819",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 660.22,
            "range": "+/- 4.435",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 581356.526,
            "range": "+/- 256.41",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9144.032,
            "range": "+/- 3.541",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 231.366,
            "range": "+/- 0.684",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2350.991,
            "range": "+/- 1.334",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 121.511,
            "range": "+/- 0.404",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36365.106,
            "range": "+/- 38.79",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1887.195,
            "range": "+/- 5.083",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1666967.298,
            "range": "+/- 18487.294",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 25922.437,
            "range": "+/- 80.395",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 678.946,
            "range": "+/- 1.422",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6704.081,
            "range": "+/- 25.773",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 292.719,
            "range": "+/- 0.617",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 102965.802,
            "range": "+/- 338.717",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5292.079,
            "range": "+/- 24.694",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5915.038,
            "range": "+/- 27.826",
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
          "id": "fd39e15b9bfde0945bc6f877e1d8ea201acf83b7",
          "message": "Merge pull request #111 from nervosys/docs/plan-phase-c-facts\n\ndocs(plan): Phase C facts, ic-mldsa is 65-only and its vectors are unpublished",
          "timestamp": "2026-09-28T14:11:42-07:00",
          "tree_id": "da3af01224f8dde3e9303970728b52f6e5ca09c9",
          "url": "https://github.com/nervosys/HyperMachine/commit/fd39e15b9bfde0945bc6f877e1d8ea201acf83b7"
        },
        "date": 1790631100672,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4647.187,
            "range": "+/- 16.399",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 16075.322,
            "range": "+/- 41.365",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4046.318,
            "range": "+/- 11.494",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6938.777,
            "range": "+/- 35.784",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3979.629,
            "range": "+/- 19.539",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 54001.195,
            "range": "+/- 467.446",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4755.473,
            "range": "+/- 13.355",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 17230.234,
            "range": "+/- 91.439",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4194.121,
            "range": "+/- 23.899",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 8393.73,
            "range": "+/- 341.814",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4051.237,
            "range": "+/- 14.975",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 53666.701,
            "range": "+/- 199.812",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 19586.021,
            "range": "+/- 78.292",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.405,
            "range": "+/- 0.269",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 122.336,
            "range": "+/- 1.287",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4891.129,
            "range": "+/- 69.961",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 7919.239,
            "range": "+/- 16.917",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 2000.112,
            "range": "+/- 26.418",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2952.16,
            "range": "+/- 32.376",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1331.737,
            "range": "+/- 18.387",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11114.704,
            "range": "+/- 20.241",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 823.795,
            "range": "+/- 9.673",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3228.235,
            "range": "+/- 6.706",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 656.138,
            "range": "+/- 3.74",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78655.767,
            "range": "+/- 934.236",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12613.2,
            "range": "+/- 67.773",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 387.342,
            "range": "+/- 2.774",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.328,
            "range": "+/- 0.047",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 99.262,
            "range": "+/- 0.318",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.795,
            "range": "+/- 0.641",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1518.022,
            "range": "+/- 5.071",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.727,
            "range": "+/- 0.077",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1979.913,
            "range": "+/- 13.649",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16652.387,
            "range": "+/- 97.102",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 746.448,
            "range": "+/- 1.106",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662957.803,
            "range": "+/- 486.108",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10440.022,
            "range": "+/- 12.243",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 260.41,
            "range": "+/- 0.816",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2697.457,
            "range": "+/- 3.045",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.224,
            "range": "+/- 0.385",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41441.904,
            "range": "+/- 39.092",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2219.34,
            "range": "+/- 8.368",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2036050.352,
            "range": "+/- 19029.684",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30409.073,
            "range": "+/- 115.48",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 808.71,
            "range": "+/- 3.879",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7893.09,
            "range": "+/- 40.409",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 349.206,
            "range": "+/- 2.244",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 121660.493,
            "range": "+/- 830.259",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 8065.126,
            "range": "+/- 104.222",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 9072.929,
            "range": "+/- 145.881",
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
          "id": "b8cdb7b88446e07d2e739ecf1b30fb2c1f48d2ab",
          "message": "Merge pull request #112 from nervosys/feat/strict-fips-enforced\n\nfeat(hv2-core): FipsMode::Strict refuses; FIPS modes self-test via ic-fips and draw from a DRBG",
          "timestamp": "2026-09-28T14:35:44-07:00",
          "tree_id": "428c1a7794061af41bcf64eb83158d82b2eccd59",
          "url": "https://github.com/nervosys/HyperMachine/commit/b8cdb7b88446e07d2e739ecf1b30fb2c1f48d2ab"
        },
        "date": 1790632506661,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4155.699,
            "range": "+/- 28.492",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 12771.898,
            "range": "+/- 76.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3694.121,
            "range": "+/- 19.805",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5795.291,
            "range": "+/- 30.254",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3570.057,
            "range": "+/- 19.539",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 41798.226,
            "range": "+/- 224.164",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 6433.12,
            "range": "+/- 29.456",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 15876.165,
            "range": "+/- 85.204",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 6038.587,
            "range": "+/- 29.483",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 8462.19,
            "range": "+/- 84.494",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 5968.725,
            "range": "+/- 31.377",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 45212.547,
            "range": "+/- 211.404",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 40906191,
            "range": "+/- 183917.503",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 2337.756,
            "range": "+/- 16.205",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 2354.523,
            "range": "+/- 12.266",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 3665.266,
            "range": "+/- 16.081",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 6605.19,
            "range": "+/- 41.825",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1492.808,
            "range": "+/- 7.698",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2261.911,
            "range": "+/- 19.528",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1110.482,
            "range": "+/- 3.317",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10306.747,
            "range": "+/- 38.024",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 672.316,
            "range": "+/- 6.203",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3022.035,
            "range": "+/- 32.271",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 553.373,
            "range": "+/- 2.924",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 57341.698,
            "range": "+/- 511.758",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8923.496,
            "range": "+/- 102.964",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 24744.541,
            "range": "+/- 109.962",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 2223.65,
            "range": "+/- 13.187",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 7296.13,
            "range": "+/- 37.445",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 2216.117,
            "range": "+/- 10.121",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 94100.78,
            "range": "+/- 371.226",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 2948.487,
            "range": "+/- 17.452",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1456.752,
            "range": "+/- 6.302",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12411.175,
            "range": "+/- 87.176",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 689.315,
            "range": "+/- 3.457",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 633752.681,
            "range": "+/- 3231.044",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9976.329,
            "range": "+/- 61.441",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.252,
            "range": "+/- 1.389",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2505.449,
            "range": "+/- 9.643",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 120.376,
            "range": "+/- 0.899",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 39525.421,
            "range": "+/- 164.757",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2198.104,
            "range": "+/- 35.899",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1856169.168,
            "range": "+/- 9017.316",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 29548.35,
            "range": "+/- 159.392",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 774.35,
            "range": "+/- 6.913",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7534.785,
            "range": "+/- 35.947",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 322.124,
            "range": "+/- 1.53",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 117335.833,
            "range": "+/- 680.032",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5067.439,
            "range": "+/- 44.245",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 6140.674,
            "range": "+/- 63.203",
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
          "id": "0e3ce2fb59cd629625b3fd86b3a23ca5a1cac519",
          "message": "Merge pull request #113 from nervosys/feat/secure-boot-verifies\n\nfeat(hv2-core): secure boot verifies signatures against the trusted key",
          "timestamp": "2026-09-28T17:41:04-07:00",
          "tree_id": "8069084bb6d717ee8c504429eb7d3fb6ff376723",
          "url": "https://github.com/nervosys/HyperMachine/commit/0e3ce2fb59cd629625b3fd86b3a23ca5a1cac519"
        },
        "date": 1790643569063,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4615.603,
            "range": "+/- 9.952",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 16326.528,
            "range": "+/- 109.592",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4092.511,
            "range": "+/- 27.416",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6883.777,
            "range": "+/- 19.769",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3925.616,
            "range": "+/- 6.535",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 53237.808,
            "range": "+/- 214.284",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 7397.706,
            "range": "+/- 33.007",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 21981.888,
            "range": "+/- 81.453",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 6808.696,
            "range": "+/- 34.33",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 9694.837,
            "range": "+/- 21.227",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 6645.079,
            "range": "+/- 18.395",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 78509.164,
            "range": "+/- 423.363",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 55962458,
            "range": "+/- 166829.455",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 2709.226,
            "range": "+/- 20.609",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 2701.746,
            "range": "+/- 7.344",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 5020.438,
            "range": "+/- 88.722",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 8442.289,
            "range": "+/- 105.568",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 2354.45,
            "range": "+/- 40.142",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 3108.142,
            "range": "+/- 56.451",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1252.438,
            "range": "+/- 3.979",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11055.664,
            "range": "+/- 15.243",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 763.484,
            "range": "+/- 2.141",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3193.243,
            "range": "+/- 4.748",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 642.584,
            "range": "+/- 2.603",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76396.358,
            "range": "+/- 979.874",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12353.729,
            "range": "+/- 42.73",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 29271.469,
            "range": "+/- 96.818",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 2658.175,
            "range": "+/- 29.036",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 8614.63,
            "range": "+/- 31.051",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 2593.256,
            "range": "+/- 6.92",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 115504.215,
            "range": "+/- 905.2",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 3943.267,
            "range": "+/- 114.943",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1997.178,
            "range": "+/- 16.824",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17889.508,
            "range": "+/- 324.288",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 748.552,
            "range": "+/- 2.023",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664972.879,
            "range": "+/- 1245.142",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10471.436,
            "range": "+/- 11.382",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 262.211,
            "range": "+/- 0.812",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2679.074,
            "range": "+/- 2.127",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.495,
            "range": "+/- 0.195",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41606.593,
            "range": "+/- 53.069",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2262.279,
            "range": "+/- 12.476",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1937239.912,
            "range": "+/- 5390.458",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30824.439,
            "range": "+/- 144.253",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 811.783,
            "range": "+/- 2.571",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7950.21,
            "range": "+/- 31.723",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 354.561,
            "range": "+/- 1.079",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 121780.762,
            "range": "+/- 393.497",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7123.812,
            "range": "+/- 32.013",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7954.473,
            "range": "+/- 24.64",
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
          "id": "c420f663128164b7cdda2da0764a150c1324d0e2",
          "message": "Merge pull request #114 from nervosys/feat/tamper-evident-audit\n\nfeat(audit): tamper-evident audit chain, fed by the MCP and HTTP audit logs",
          "timestamp": "2026-09-28T19:58:24-07:00",
          "tree_id": "899aeba0ec5444392e89f21800eb9cf2b913ca76",
          "url": "https://github.com/nervosys/HyperMachine/commit/c420f663128164b7cdda2da0764a150c1324d0e2"
        },
        "date": 1790651762234,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 5054.115,
            "range": "+/- 14.319",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 17569.847,
            "range": "+/- 54.344",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4429.905,
            "range": "+/- 7.717",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 7506.469,
            "range": "+/- 14.024",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4292.833,
            "range": "+/- 6.179",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 57933.442,
            "range": "+/- 240.603",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 8087.046,
            "range": "+/- 21.056",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 21365.36,
            "range": "+/- 102.839",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 7445.671,
            "range": "+/- 17.311",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 10618.798,
            "range": "+/- 23.721",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 7320.488,
            "range": "+/- 24.285",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 60757.928,
            "range": "+/- 204.688",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 61128336,
            "range": "+/- 130771.488",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 2969.148,
            "range": "+/- 9.05",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 3034.256,
            "range": "+/- 20.577",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 5432.513,
            "range": "+/- 77.105",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 8632.06,
            "range": "+/- 18.947",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 2227.937,
            "range": "+/- 31.8",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 3169.35,
            "range": "+/- 30.94",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1437.598,
            "range": "+/- 9.453",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12533.436,
            "range": "+/- 23.044",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 878.099,
            "range": "+/- 4.957",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3613.915,
            "range": "+/- 6.7",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 732.824,
            "range": "+/- 6.809",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77688.306,
            "range": "+/- 933.072",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11555.784,
            "range": "+/- 104.505",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 32637.567,
            "range": "+/- 341.953",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 2868.063,
            "range": "+/- 7.407",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 9545.438,
            "range": "+/- 48.282",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 3046.213,
            "range": "+/- 53.111",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 122221.161,
            "range": "+/- 483.991",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 3796.811,
            "range": "+/- 9.685",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2051.365,
            "range": "+/- 17.495",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16113.502,
            "range": "+/- 180.653",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.223,
            "range": "+/- 0.762",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 748639.029,
            "range": "+/- 178.447",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11835.464,
            "range": "+/- 6.003",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 292.302,
            "range": "+/- 0.683",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3042.732,
            "range": "+/- 1.983",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 155.741,
            "range": "+/- 0.913",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46920.239,
            "range": "+/- 37.403",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2473.4,
            "range": "+/- 12.128",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2121199.667,
            "range": "+/- 5119.666",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33946.909,
            "range": "+/- 211.596",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 967.198,
            "range": "+/- 17.173",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8753.734,
            "range": "+/- 32.102",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 390.483,
            "range": "+/- 2.339",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 133864.796,
            "range": "+/- 515.421",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7080.869,
            "range": "+/- 81.329",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7833.309,
            "range": "+/- 88.384",
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
          "id": "539d57c9c75a231289ac8f964c59db34f60b9d49",
          "message": "Merge pull request #115 from nervosys/feat/serve-audit-chain\n\nfeat(hv2-cli): hv2 serve writes the audit chain; correct #114's wrong claim",
          "timestamp": "2026-09-28T21:00:57-07:00",
          "tree_id": "2cee8fc7c0f17c97b32bac9e6f817c49d2771c90",
          "url": "https://github.com/nervosys/HyperMachine/commit/539d57c9c75a231289ac8f964c59db34f60b9d49"
        },
        "date": 1790655553808,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4740.951,
            "range": "+/- 33.097",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 16122.105,
            "range": "+/- 46.621",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4204.134,
            "range": "+/- 39.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6894.722,
            "range": "+/- 23.944",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4088.708,
            "range": "+/- 33.863",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 54158.263,
            "range": "+/- 553.067",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 7834.589,
            "range": "+/- 97.975",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 22706.831,
            "range": "+/- 236.64",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 7066.24,
            "range": "+/- 73.389",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 10260.122,
            "range": "+/- 98.956",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 6918.932,
            "range": "+/- 66.391",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 58503.167,
            "range": "+/- 541.422",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 55502105,
            "range": "+/- 117773.746",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 2703.064,
            "range": "+/- 10.1",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 2726.981,
            "range": "+/- 10.701",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 4447.744,
            "range": "+/- 32.786",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 7929.713,
            "range": "+/- 42.458",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 1791.316,
            "range": "+/- 3.94",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 2695.122,
            "range": "+/- 16.491",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1274.929,
            "range": "+/- 5.992",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10946.166,
            "range": "+/- 10.342",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 789.289,
            "range": "+/- 5.738",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 8756.855,
            "range": "+/- 4174.242",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 672.027,
            "range": "+/- 6.831",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72699.359,
            "range": "+/- 491.081",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12248.335,
            "range": "+/- 64.946",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 29331.835,
            "range": "+/- 110.183",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 2622.791,
            "range": "+/- 10.684",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 8636.912,
            "range": "+/- 23.881",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 2633.907,
            "range": "+/- 12.329",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 112553.562,
            "range": "+/- 554.602",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 3480.33,
            "range": "+/- 12.227",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1995.608,
            "range": "+/- 6.542",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16018.388,
            "range": "+/- 38.408",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 753.361,
            "range": "+/- 1.56",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664289.557,
            "range": "+/- 806.692",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10473.996,
            "range": "+/- 10.925",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 267.721,
            "range": "+/- 1.034",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2704.258,
            "range": "+/- 4.786",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 382.436,
            "range": "+/- 65.603",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41626.507,
            "range": "+/- 60.584",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2330.799,
            "range": "+/- 23.677",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2071240.8,
            "range": "+/- 28406.554",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 32539.956,
            "range": "+/- 364.729",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 850.007,
            "range": "+/- 9.291",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8637.539,
            "range": "+/- 162.376",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 375.523,
            "range": "+/- 5.822",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 126038.766,
            "range": "+/- 1443.851",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7164.99,
            "range": "+/- 47.042",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7925.209,
            "range": "+/- 29.994",
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
          "id": "e9776a05a3d94ea02029b10f06b530bf99ad46e8",
          "message": "Merge pull request #116 from nervosys/deps/ironcrypto-0.2\n\ndeps: IronCrypto 0.2.2, and RSA-4096 works again",
          "timestamp": "2026-09-29T07:50:04-07:00",
          "tree_id": "7aa89c8875045af8d8b6f8dffe17f2a1533921bd",
          "url": "https://github.com/nervosys/HyperMachine/commit/e9776a05a3d94ea02029b10f06b530bf99ad46e8"
        },
        "date": 1790694515297,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3840.811,
            "range": "+/- 18.604",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8949.105,
            "range": "+/- 44.25",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3560.209,
            "range": "+/- 18.197",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4811.189,
            "range": "+/- 16.844",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3472.512,
            "range": "+/- 17.638",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26615.509,
            "range": "+/- 246.296",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4708.606,
            "range": "+/- 17.253",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 10479.349,
            "range": "+/- 61.848",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4458.002,
            "range": "+/- 23.277",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 5812.184,
            "range": "+/- 24.706",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4339.149,
            "range": "+/- 18.463",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 28392.801,
            "range": "+/- 123.788",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 30125131,
            "range": "+/- 142109.127",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 817.137,
            "range": "+/- 3.567",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 825.389,
            "range": "+/- 4.186",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1287.127,
            "range": "+/- 14.873",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2242.625,
            "range": "+/- 9.864",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 536.596,
            "range": "+/- 2.645",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 781.247,
            "range": "+/- 3.438",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 863.599,
            "range": "+/- 3.795",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10254.717,
            "range": "+/- 64.175",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 398.706,
            "range": "+/- 2.148",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2687.976,
            "range": "+/- 12.366",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 278.475,
            "range": "+/- 1.554",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 62516.706,
            "range": "+/- 417.921",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8649.404,
            "range": "+/- 42.715",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7777.722,
            "range": "+/- 30.153",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 766.239,
            "range": "+/- 5.954",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2337.028,
            "range": "+/- 9.981",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 762.529,
            "range": "+/- 7.882",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 29774.062,
            "range": "+/- 138.404",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 987.021,
            "range": "+/- 6.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1459.591,
            "range": "+/- 9.585",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12076.23,
            "range": "+/- 50.456",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 664.213,
            "range": "+/- 2.596",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 619147.742,
            "range": "+/- 1948.452",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9833.397,
            "range": "+/- 39.768",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 219.57,
            "range": "+/- 1.801",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2506.761,
            "range": "+/- 14.759",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 97.076,
            "range": "+/- 0.715",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 39014.167,
            "range": "+/- 180.654",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2151.572,
            "range": "+/- 30.236",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1865255.137,
            "range": "+/- 8835.178",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30632.065,
            "range": "+/- 490.373",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 731.486,
            "range": "+/- 3.159",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7789.086,
            "range": "+/- 66.898",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 301.797,
            "range": "+/- 1.048",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 121720.51,
            "range": "+/- 1749.189",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5088.51,
            "range": "+/- 34.209",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5706.405,
            "range": "+/- 37.041",
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
          "id": "edbff50c212dfd46ea0fbcb93310cfda232ce2e0",
          "message": "Merge pull request #121 from nervosys/fix/agentic-auth-writes\n\nAPI auth: excluded paths no longer exempt mutating requests",
          "timestamp": "2026-09-29T11:51:41-07:00",
          "tree_id": "3597e354c5ad8b4889bbfa28fae6778c58fe28eb",
          "url": "https://github.com/nervosys/HyperMachine/commit/edbff50c212dfd46ea0fbcb93310cfda232ce2e0"
        },
        "date": 1790709035690,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4349.436,
            "range": "+/- 31.563",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9689.17,
            "range": "+/- 52.817",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3966.887,
            "range": "+/- 5.054",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5329.64,
            "range": "+/- 25.696",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3917.102,
            "range": "+/- 8.655",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27406.394,
            "range": "+/- 132.92",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5349.458,
            "range": "+/- 45.911",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11332.435,
            "range": "+/- 55.785",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5653.689,
            "range": "+/- 76.461",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6376.426,
            "range": "+/- 21.837",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4922.768,
            "range": "+/- 16.501",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 27974.862,
            "range": "+/- 86.736",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44434652,
            "range": "+/- 101462.034",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 936.857,
            "range": "+/- 6.998",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 924.245,
            "range": "+/- 1.931",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1455.261,
            "range": "+/- 5.947",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2557.052,
            "range": "+/- 8.885",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 619.098,
            "range": "+/- 2.873",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 892.517,
            "range": "+/- 2.317",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 916.069,
            "range": "+/- 2.201",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10628.442,
            "range": "+/- 8.405",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 426.22,
            "range": "+/- 0.634",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2857.915,
            "range": "+/- 2.932",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 305.231,
            "range": "+/- 0.569",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73169.872,
            "range": "+/- 271.317",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12355.486,
            "range": "+/- 106.941",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9147.036,
            "range": "+/- 25.644",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 836.105,
            "range": "+/- 3.397",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2716.475,
            "range": "+/- 8.386",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 833.245,
            "range": "+/- 2.583",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35504.283,
            "range": "+/- 198.402",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1104.16,
            "range": "+/- 3.046",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2073.869,
            "range": "+/- 19.083",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16227.311,
            "range": "+/- 74.149",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 723.713,
            "range": "+/- 1.922",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 671345.661,
            "range": "+/- 2928.287",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10582.809,
            "range": "+/- 29.295",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.357,
            "range": "+/- 0.526",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2657.841,
            "range": "+/- 2.314",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 116.361,
            "range": "+/- 0.354",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41566.917,
            "range": "+/- 41.764",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2596.59,
            "range": "+/- 39.539",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1948363.288,
            "range": "+/- 8526.257",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 36422.406,
            "range": "+/- 704.555",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 903.765,
            "range": "+/- 17.652",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8941.372,
            "range": "+/- 137.583",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 359.247,
            "range": "+/- 4.188",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 129049.55,
            "range": "+/- 2350.797",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7143.093,
            "range": "+/- 58.837",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8014.303,
            "range": "+/- 16.521",
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
          "id": "53e7e25ca39205618bcf9ad9992c4eb15ad79142",
          "message": "Merge pull request #123 from nervosys/feat/sandbox-env\n\nSandbox envVars, as E2B's NewSandbox has them",
          "timestamp": "2026-09-29T12:59:36-07:00",
          "tree_id": "2d09a16e9551102f0f708ded91950a8db0f9c393",
          "url": "https://github.com/nervosys/HyperMachine/commit/53e7e25ca39205618bcf9ad9992c4eb15ad79142"
        },
        "date": 1790713166522,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4260.084,
            "range": "+/- 11.941",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9588.561,
            "range": "+/- 28.368",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3984.903,
            "range": "+/- 16.94",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5303.101,
            "range": "+/- 22.664",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3918.147,
            "range": "+/- 9.243",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27077.429,
            "range": "+/- 70.636",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5224.424,
            "range": "+/- 17.918",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12187.252,
            "range": "+/- 55.392",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4926.243,
            "range": "+/- 8.731",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6345.628,
            "range": "+/- 25.449",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4870.488,
            "range": "+/- 23.745",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29452.709,
            "range": "+/- 104.472",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44590463,
            "range": "+/- 109272.517",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 914.449,
            "range": "+/- 3.038",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 931.863,
            "range": "+/- 4.827",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1530.363,
            "range": "+/- 13.473",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2547.097,
            "range": "+/- 5.485",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.622,
            "range": "+/- 4.672",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 949.076,
            "range": "+/- 7.493",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 912.313,
            "range": "+/- 1.577",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10704.11,
            "range": "+/- 11.29",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 427.164,
            "range": "+/- 0.895",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2876.989,
            "range": "+/- 7.291",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 306.569,
            "range": "+/- 0.758",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72002.287,
            "range": "+/- 463.273",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11197.549,
            "range": "+/- 80.186",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9280.431,
            "range": "+/- 33.86",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 846.428,
            "range": "+/- 2.178",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2739.363,
            "range": "+/- 12.12",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 849.954,
            "range": "+/- 4.103",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35175.711,
            "range": "+/- 159.834",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1220.926,
            "range": "+/- 27.88",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1943.21,
            "range": "+/- 7.544",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 14928.157,
            "range": "+/- 48.538",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 723.041,
            "range": "+/- 0.624",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662910.827,
            "range": "+/- 407.313",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10407.234,
            "range": "+/- 6.961",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.845,
            "range": "+/- 0.277",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2657.25,
            "range": "+/- 1.629",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 116.852,
            "range": "+/- 0.521",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41397.483,
            "range": "+/- 22.294",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2367.874,
            "range": "+/- 61.946",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1956744.885,
            "range": "+/- 10204.298",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30894.19,
            "range": "+/- 150.081",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 799.815,
            "range": "+/- 3.283",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8254.727,
            "range": "+/- 78.917",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 342.4,
            "range": "+/- 1.563",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 133356.549,
            "range": "+/- 1857.939",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6688.924,
            "range": "+/- 72.912",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7491.828,
            "range": "+/- 90.714",
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
          "id": "60ac2027feb9e4797c2488b69ffddffbcac4b0ca",
          "message": "Merge pull request #124 from nervosys/feat/idle-pause\n\nIdle pause: unused sandboxes pause to disk, and wake on traffic",
          "timestamp": "2026-09-29T18:27:56-07:00",
          "tree_id": "6ca6b5d7f8865ef349305e4fd7f6f6f8e18c7101",
          "url": "https://github.com/nervosys/HyperMachine/commit/60ac2027feb9e4797c2488b69ffddffbcac4b0ca"
        },
        "date": 1790732755285,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4268.048,
            "range": "+/- 20.929",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9622.81,
            "range": "+/- 48.213",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3983.063,
            "range": "+/- 12.833",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5344.422,
            "range": "+/- 28.645",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3915.2,
            "range": "+/- 12.593",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27336.912,
            "range": "+/- 155.748",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5239.085,
            "range": "+/- 33.797",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12277.998,
            "range": "+/- 60.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4937.384,
            "range": "+/- 10.449",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6402.783,
            "range": "+/- 44.526",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4852.939,
            "range": "+/- 12.539",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29870.971,
            "range": "+/- 155.027",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44722900,
            "range": "+/- 111092.434",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 929.606,
            "range": "+/- 2.613",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 939.63,
            "range": "+/- 2.928",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1449.88,
            "range": "+/- 3.886",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2551.409,
            "range": "+/- 5.399",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 656.476,
            "range": "+/- 5.105",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 966.664,
            "range": "+/- 11.335",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 921.453,
            "range": "+/- 2.834",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10650.589,
            "range": "+/- 12.155",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 426.651,
            "range": "+/- 0.755",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2894.228,
            "range": "+/- 6.975",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 304.645,
            "range": "+/- 0.808",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77936.398,
            "range": "+/- 1144.563",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11841.753,
            "range": "+/- 236.94",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9216.208,
            "range": "+/- 27.769",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 866.186,
            "range": "+/- 4.877",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2864.505,
            "range": "+/- 24.425",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 853.342,
            "range": "+/- 3.046",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35138.403,
            "range": "+/- 132.159",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1116.26,
            "range": "+/- 3.695",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2078.533,
            "range": "+/- 28.705",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15307.4,
            "range": "+/- 212.276",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 723.347,
            "range": "+/- 0.702",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 665236.146,
            "range": "+/- 445.812",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10393.497,
            "range": "+/- 7.143",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.979,
            "range": "+/- 0.321",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2656.302,
            "range": "+/- 1.837",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 116,
            "range": "+/- 0.291",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41443.607,
            "range": "+/- 71.373",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2213.884,
            "range": "+/- 6.224",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1944762.648,
            "range": "+/- 8229.078",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30930.585,
            "range": "+/- 281.382",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 802.238,
            "range": "+/- 4.118",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8012.028,
            "range": "+/- 67.643",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 346.565,
            "range": "+/- 2.432",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 123261.755,
            "range": "+/- 1073.928",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6706.09,
            "range": "+/- 54.422",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7826.619,
            "range": "+/- 95.655",
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
          "id": "c7a7c7bc878a9cfacfb33f890e1b965185ece9c2",
          "message": "Merge pull request #125 from nervosys/feat/checkpoints\n\nCheckpoints: save a running sandbox, and roll it back in place",
          "timestamp": "2026-09-29T19:20:20-07:00",
          "tree_id": "c87936670b284520dbaf5e614be2daa1ba99f0ce",
          "url": "https://github.com/nervosys/HyperMachine/commit/c7a7c7bc878a9cfacfb33f890e1b965185ece9c2"
        },
        "date": 1790735953812,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4322.771,
            "range": "+/- 22.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9601.816,
            "range": "+/- 40.954",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3980.486,
            "range": "+/- 12.192",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5285.42,
            "range": "+/- 11.986",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3960.306,
            "range": "+/- 19.02",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 29449.289,
            "range": "+/- 73.431",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5211.016,
            "range": "+/- 15.971",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12490.911,
            "range": "+/- 60.272",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4958.982,
            "range": "+/- 19.866",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6331.849,
            "range": "+/- 34.152",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4846.846,
            "range": "+/- 13.812",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 69050.797,
            "range": "+/- 2323.072",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45172074.5,
            "range": "+/- 97618.791",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 937.981,
            "range": "+/- 12.581",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 925.904,
            "range": "+/- 3.571",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1456.449,
            "range": "+/- 3.893",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2589.206,
            "range": "+/- 11.081",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 620.472,
            "range": "+/- 1.242",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 964.219,
            "range": "+/- 19.554",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 953.357,
            "range": "+/- 6.901",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10661.026,
            "range": "+/- 17.721",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 449.642,
            "range": "+/- 2.433",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2990.557,
            "range": "+/- 34.563",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 324.167,
            "range": "+/- 2.598",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75265.559,
            "range": "+/- 708.036",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11972.934,
            "range": "+/- 195.135",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9259.242,
            "range": "+/- 50.503",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 852.773,
            "range": "+/- 6.495",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2731.36,
            "range": "+/- 8.563",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 837.046,
            "range": "+/- 2.59",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35713.375,
            "range": "+/- 202.713",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1109.575,
            "range": "+/- 4.117",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2027.469,
            "range": "+/- 28.618",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15477.612,
            "range": "+/- 125.899",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 725.31,
            "range": "+/- 2.253",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662512.151,
            "range": "+/- 381.327",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10436.859,
            "range": "+/- 11.534",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 236.692,
            "range": "+/- 0.247",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2663.269,
            "range": "+/- 1.763",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 116.881,
            "range": "+/- 0.455",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41451.743,
            "range": "+/- 18.265",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2296.295,
            "range": "+/- 20.359",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2239519.28,
            "range": "+/- 61505.142",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31792.642,
            "range": "+/- 267.872",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 797.264,
            "range": "+/- 4.712",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7924.476,
            "range": "+/- 19.666",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 339.707,
            "range": "+/- 1.375",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 133135.18,
            "range": "+/- 1497.298",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6974.361,
            "range": "+/- 90.072",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8051.723,
            "range": "+/- 178.081",
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
          "id": "7bd3bab4c3b1b3b9eb5d3a284edd9c6815f4b349",
          "message": "Merge pull request #126 from nervosys/feat/hm-sandbox-run\n\nhm sandbox run, and streaming and cancellation in hv2-sandbox",
          "timestamp": "2026-09-29T20:09:46-07:00",
          "tree_id": "c61e4f4849d44b4bd61dce8ce7c5c6d73825f8f6",
          "url": "https://github.com/nervosys/HyperMachine/commit/7bd3bab4c3b1b3b9eb5d3a284edd9c6815f4b349"
        },
        "date": 1790738776206,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4257.111,
            "range": "+/- 17.558",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9658.335,
            "range": "+/- 35.414",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3991.296,
            "range": "+/- 10.593",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5410.21,
            "range": "+/- 40.138",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3963.607,
            "range": "+/- 26.182",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27275.522,
            "range": "+/- 133.187",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5245.147,
            "range": "+/- 16.868",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13772.381,
            "range": "+/- 73.631",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4957.706,
            "range": "+/- 21.265",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6366.84,
            "range": "+/- 25.701",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4932.782,
            "range": "+/- 28.322",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 31564.567,
            "range": "+/- 121.244",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44421095,
            "range": "+/- 107084.852",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 913.507,
            "range": "+/- 2.634",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 936.161,
            "range": "+/- 3.607",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1451.803,
            "range": "+/- 3.518",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2563.94,
            "range": "+/- 7.744",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 630.274,
            "range": "+/- 5.955",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 894.839,
            "range": "+/- 2.715",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 914.475,
            "range": "+/- 1.647",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10933.294,
            "range": "+/- 109.526",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 448.269,
            "range": "+/- 2.688",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2856.084,
            "range": "+/- 2.949",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 335.98,
            "range": "+/- 3.091",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77288.216,
            "range": "+/- 1210.188",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11426.993,
            "range": "+/- 163.789",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9238.727,
            "range": "+/- 37.815",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 837.583,
            "range": "+/- 3.158",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2746.561,
            "range": "+/- 12.721",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 842.936,
            "range": "+/- 4.551",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35326.857,
            "range": "+/- 229.846",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1103.356,
            "range": "+/- 3.251",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2101.814,
            "range": "+/- 32.364",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17859.532,
            "range": "+/- 1431.779",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 719.677,
            "range": "+/- 0.371",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 666171.153,
            "range": "+/- 704.016",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10410.954,
            "range": "+/- 6.26",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.333,
            "range": "+/- 0.499",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2666.789,
            "range": "+/- 6.509",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 116.409,
            "range": "+/- 0.44",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 42592.047,
            "range": "+/- 420.251",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2294.734,
            "range": "+/- 37.024",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2153321.304,
            "range": "+/- 27715.856",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33231.968,
            "range": "+/- 550.624",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 899.09,
            "range": "+/- 17.576",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8919.29,
            "range": "+/- 107.517",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 342.514,
            "range": "+/- 1.947",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 132549.135,
            "range": "+/- 1609.516",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6462.297,
            "range": "+/- 48.713",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7573.802,
            "range": "+/- 128.773",
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
          "id": "2cf699502f3bc332d20749da5a2648e1675e3c64",
          "message": "Merge pull request #127 from nervosys/feat/hm-jobs\n\nhm jobs: a durable queue of sandboxed host jobs",
          "timestamp": "2026-09-29T21:13:52-07:00",
          "tree_id": "146418840f7491b2489b076af2682ccca9de1b5a",
          "url": "https://github.com/nervosys/HyperMachine/commit/2cf699502f3bc332d20749da5a2648e1675e3c64"
        },
        "date": 1790742713812,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4258.457,
            "range": "+/- 13.64",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9740.664,
            "range": "+/- 55.486",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4008.147,
            "range": "+/- 17.492",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5304.657,
            "range": "+/- 19.64",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4335.34,
            "range": "+/- 85.072",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27156.215,
            "range": "+/- 121.43",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5223.948,
            "range": "+/- 16.642",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11892.608,
            "range": "+/- 113.631",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4939.749,
            "range": "+/- 17.498",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6414.578,
            "range": "+/- 43.9",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4858.39,
            "range": "+/- 17.08",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 30803.984,
            "range": "+/- 309.512",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44595587.5,
            "range": "+/- 119916.997",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 931.07,
            "range": "+/- 3.242",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 951.375,
            "range": "+/- 3.7",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1479.673,
            "range": "+/- 4.873",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2621.681,
            "range": "+/- 9.629",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 620.605,
            "range": "+/- 3.54",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 887.907,
            "range": "+/- 2.183",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 923.345,
            "range": "+/- 3.521",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10649.307,
            "range": "+/- 27.121",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 424.396,
            "range": "+/- 1.056",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3287.157,
            "range": "+/- 120.651",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 302.613,
            "range": "+/- 0.738",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76542.773,
            "range": "+/- 949.218",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12098.584,
            "range": "+/- 230.25",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9252.699,
            "range": "+/- 28.753",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 831.613,
            "range": "+/- 2.582",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2746.911,
            "range": "+/- 9.775",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 847.7,
            "range": "+/- 3.804",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35201.824,
            "range": "+/- 79.645",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1112.238,
            "range": "+/- 4.041",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1984.261,
            "range": "+/- 9.783",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16080.242,
            "range": "+/- 190.188",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 723.178,
            "range": "+/- 1.19",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663908.116,
            "range": "+/- 756.658",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10475.572,
            "range": "+/- 13.67",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 237.428,
            "range": "+/- 0.327",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2677.971,
            "range": "+/- 7.173",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 115.967,
            "range": "+/- 0.159",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41585.674,
            "range": "+/- 74.028",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2456.14,
            "range": "+/- 36.507",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2012535.25,
            "range": "+/- 18395.073",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33919.128,
            "range": "+/- 428.593",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 850.383,
            "range": "+/- 10.935",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8992.532,
            "range": "+/- 170.631",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 338.391,
            "range": "+/- 1.195",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 135170.769,
            "range": "+/- 1759.845",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6344.17,
            "range": "+/- 16.627",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7323.12,
            "range": "+/- 74.626",
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
          "id": "ec95c3bbad55ecf47ac70316306a4d87d87fbb33",
          "message": "Merge pull request #141 from nervosys/fix/duplicate-snapshot-test\n\nRemove a duplicate snapshot test that broke every test build",
          "timestamp": "2026-10-06T09:34:45-07:00",
          "tree_id": "88a38caada5bed291261210914f9a0c17b6124c5",
          "url": "https://github.com/nervosys/HyperMachine/commit/ec95c3bbad55ecf47ac70316306a4d87d87fbb33"
        },
        "date": 1791307864662,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4341.285,
            "range": "+/- 26.668",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9665.079,
            "range": "+/- 37.843",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4067.63,
            "range": "+/- 21.614",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5366.666,
            "range": "+/- 38.837",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3929.086,
            "range": "+/- 18.461",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27083.368,
            "range": "+/- 118.382",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5223.34,
            "range": "+/- 18.898",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13248.056,
            "range": "+/- 235.224",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5456.836,
            "range": "+/- 78.438",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6321.547,
            "range": "+/- 15.818",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 5277.638,
            "range": "+/- 43.643",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 49483.819,
            "range": "+/- 410.809",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45569304,
            "range": "+/- 161069.211",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 905.498,
            "range": "+/- 3.58",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 915.289,
            "range": "+/- 2.72",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1464.55,
            "range": "+/- 5.098",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2576.193,
            "range": "+/- 7.794",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 619.592,
            "range": "+/- 3.541",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 919.043,
            "range": "+/- 8.144",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 925.284,
            "range": "+/- 3.348",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10611.987,
            "range": "+/- 5.424",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 429.267,
            "range": "+/- 1.567",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2870.522,
            "range": "+/- 5.18",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 307.038,
            "range": "+/- 0.942",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 71788.853,
            "range": "+/- 464.641",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13447.616,
            "range": "+/- 215.193",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9007.56,
            "range": "+/- 18.888",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 823.118,
            "range": "+/- 3.127",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2684.445,
            "range": "+/- 14.154",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 824.15,
            "range": "+/- 3.132",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34463.912,
            "range": "+/- 98.205",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1089.098,
            "range": "+/- 3.87",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2092.167,
            "range": "+/- 37.948",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17306.271,
            "range": "+/- 199.479",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 724.829,
            "range": "+/- 0.664",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 665323.569,
            "range": "+/- 480.077",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10468.943,
            "range": "+/- 14.591",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 240.29,
            "range": "+/- 0.253",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2664.704,
            "range": "+/- 2.185",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 120.133,
            "range": "+/- 0.194",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41579.814,
            "range": "+/- 39.052",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2225.014,
            "range": "+/- 6.522",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1977764.068,
            "range": "+/- 10851.266",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31469.974,
            "range": "+/- 348.079",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 798.776,
            "range": "+/- 2.977",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7978.498,
            "range": "+/- 44.427",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 342.565,
            "range": "+/- 1.133",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 122878.936,
            "range": "+/- 624.366",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7759.413,
            "range": "+/- 138.278",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8604.169,
            "range": "+/- 107.114",
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
          "id": "1b8f79555f9d146b35487428b850dd5cbdca5676",
          "message": "Merge pull request #142 from nervosys/fix/flaky-dual-bind-test\n\nFix two CI failures on master: a flaky port test and stale coverage profiles",
          "timestamp": "2026-10-06T11:25:46-07:00",
          "tree_id": "f78bb1f8392926914859345875aea7068e08c03b",
          "url": "https://github.com/nervosys/HyperMachine/commit/1b8f79555f9d146b35487428b850dd5cbdca5676"
        },
        "date": 1791314321463,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4254.895,
            "range": "+/- 10.555",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9677.508,
            "range": "+/- 88.769",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3986.583,
            "range": "+/- 9.087",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5275.651,
            "range": "+/- 21.671",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3924.121,
            "range": "+/- 18.416",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 28017.792,
            "range": "+/- 257.153",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5244.483,
            "range": "+/- 10.953",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13776.551,
            "range": "+/- 52.302",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4991.129,
            "range": "+/- 14.177",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6430.122,
            "range": "+/- 56.213",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4884.193,
            "range": "+/- 12.325",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 50511.093,
            "range": "+/- 316.307",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45102871.5,
            "range": "+/- 123601.194",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 920.593,
            "range": "+/- 3.464",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 940.301,
            "range": "+/- 6.785",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1425.728,
            "range": "+/- 3.892",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2520.664,
            "range": "+/- 4.929",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.349,
            "range": "+/- 5.015",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 935.608,
            "range": "+/- 10.534",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 920.124,
            "range": "+/- 1.97",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10668.529,
            "range": "+/- 11.393",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 431.721,
            "range": "+/- 1.968",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2881.865,
            "range": "+/- 4.223",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 304.434,
            "range": "+/- 0.783",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75610.681,
            "range": "+/- 288.529",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12212.068,
            "range": "+/- 51.168",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9060.24,
            "range": "+/- 26.649",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 851.026,
            "range": "+/- 6.217",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2695.089,
            "range": "+/- 9.682",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 845.503,
            "range": "+/- 4.716",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34705.516,
            "range": "+/- 132.372",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1096.773,
            "range": "+/- 3.751",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2013.277,
            "range": "+/- 10.025",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16245.944,
            "range": "+/- 65.59",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 724.963,
            "range": "+/- 0.587",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664535.874,
            "range": "+/- 739.696",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10407.385,
            "range": "+/- 8.328",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.118,
            "range": "+/- 0.281",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2659.524,
            "range": "+/- 1.394",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.796,
            "range": "+/- 0.241",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41389.161,
            "range": "+/- 27.281",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2257.342,
            "range": "+/- 6.482",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1978830.647,
            "range": "+/- 10014.778",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 32324.612,
            "range": "+/- 394.191",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 808.894,
            "range": "+/- 4.755",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8174.968,
            "range": "+/- 101.178",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 335.67,
            "range": "+/- 1.147",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 123466.831,
            "range": "+/- 465.544",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7192.753,
            "range": "+/- 46.161",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8059.303,
            "range": "+/- 51.208",
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
          "id": "1fdc417002e8ef372bfbcaaa9b0e1b1b8151f7b0",
          "message": "Merge pull request #137 from nervosys/dependabot/cargo/ruzstd-0.9.0\n\ndeps(deps): bump ruzstd from 0.8.3 to 0.9.0",
          "timestamp": "2026-10-06T13:17:11-07:00",
          "tree_id": "6a5e00d80026aeb4f5c7ce699b58328b6608ffd2",
          "url": "https://github.com/nervosys/HyperMachine/commit/1fdc417002e8ef372bfbcaaa9b0e1b1b8151f7b0"
        },
        "date": 1791318876208,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4910.26,
            "range": "+/- 70.2",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9646.402,
            "range": "+/- 38.17",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4064.5,
            "range": "+/- 18.781",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5406.915,
            "range": "+/- 33.644",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4140.936,
            "range": "+/- 80.797",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26943.86,
            "range": "+/- 136.2",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5196.136,
            "range": "+/- 17.116",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 14335.87,
            "range": "+/- 38.91",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4904.729,
            "range": "+/- 12.076",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6330.738,
            "range": "+/- 21.052",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4841.91,
            "range": "+/- 20.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 28196.119,
            "range": "+/- 256.538",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43111326.5,
            "range": "+/- 80748.483",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 891.568,
            "range": "+/- 2.102",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 908.098,
            "range": "+/- 5.254",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1519.828,
            "range": "+/- 17.431",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2543.316,
            "range": "+/- 12.749",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 614.976,
            "range": "+/- 4.452",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 919.345,
            "range": "+/- 5.815",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 938.938,
            "range": "+/- 1.608",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10701.736,
            "range": "+/- 11.062",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 420.158,
            "range": "+/- 0.49",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2909.484,
            "range": "+/- 8.841",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 300.944,
            "range": "+/- 0.838",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78809.651,
            "range": "+/- 297.998",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12704.055,
            "range": "+/- 171.206",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9143.937,
            "range": "+/- 35.118",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 821.891,
            "range": "+/- 1.999",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2694.887,
            "range": "+/- 13.491",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 833.978,
            "range": "+/- 5.051",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34556.634,
            "range": "+/- 74.093",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1090.889,
            "range": "+/- 3.612",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2102.359,
            "range": "+/- 12.444",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16647.403,
            "range": "+/- 114.675",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 724.134,
            "range": "+/- 0.957",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662614.53,
            "range": "+/- 485.918",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10446.492,
            "range": "+/- 7.774",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 240.963,
            "range": "+/- 0.547",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2660.091,
            "range": "+/- 1.504",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.202,
            "range": "+/- 0.169",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41404.414,
            "range": "+/- 52.839",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2204.123,
            "range": "+/- 15.512",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1920530.169,
            "range": "+/- 7941.998",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30219.776,
            "range": "+/- 81.714",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 794.639,
            "range": "+/- 4.006",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7812.956,
            "range": "+/- 33.614",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 346.587,
            "range": "+/- 2.677",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124480.6,
            "range": "+/- 1044.378",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7684.43,
            "range": "+/- 81.342",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8392.565,
            "range": "+/- 83.161",
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
          "id": "442bb9249635e67012b4094612a840b9bcb78ac6",
          "message": "Merge pull request #143 from nervosys/perf/guest-thp\n\nBack guest RAM with huge pages: 88% fewer VM exits per boot",
          "timestamp": "2026-10-06T15:44:25-07:00",
          "tree_id": "e2c473f68f5c1c3fc35cb02809771ba3cbcadc33",
          "url": "https://github.com/nervosys/HyperMachine/commit/442bb9249635e67012b4094612a840b9bcb78ac6"
        },
        "date": 1791327680054,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4265.108,
            "range": "+/- 24.26",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9550.433,
            "range": "+/- 31.272",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3980.068,
            "range": "+/- 6.064",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5281.371,
            "range": "+/- 16.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3902.748,
            "range": "+/- 6.296",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26952.829,
            "range": "+/- 135.206",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5235.255,
            "range": "+/- 17.011",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13841.977,
            "range": "+/- 51.226",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4965.071,
            "range": "+/- 19.028",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6327.591,
            "range": "+/- 14.303",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4877.555,
            "range": "+/- 25.098",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 30870.483,
            "range": "+/- 552.218",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43302763,
            "range": "+/- 78506.577",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 938.195,
            "range": "+/- 4.451",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 946.539,
            "range": "+/- 4.031",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1501.09,
            "range": "+/- 11.155",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2707.396,
            "range": "+/- 22.357",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 599.369,
            "range": "+/- 2.198",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 877.577,
            "range": "+/- 2.93",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 912.835,
            "range": "+/- 2.464",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10676.466,
            "range": "+/- 15.59",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 424.302,
            "range": "+/- 1.101",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2872.782,
            "range": "+/- 6.855",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 302,
            "range": "+/- 1.041",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74793.78,
            "range": "+/- 814.515",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12592.432,
            "range": "+/- 85.285",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9472.014,
            "range": "+/- 196.86",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 877.27,
            "range": "+/- 7.274",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2683.562,
            "range": "+/- 9.437",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 937.053,
            "range": "+/- 13.266",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34107.031,
            "range": "+/- 73.749",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1098.681,
            "range": "+/- 3.763",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2054.427,
            "range": "+/- 25.399",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17873.521,
            "range": "+/- 171.201",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 730.154,
            "range": "+/- 1.157",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662758.592,
            "range": "+/- 639.903",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10453.826,
            "range": "+/- 6.906",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.626,
            "range": "+/- 0.518",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2683.18,
            "range": "+/- 4.473",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.505,
            "range": "+/- 0.312",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41744.198,
            "range": "+/- 138.868",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2226.114,
            "range": "+/- 7.858",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1949997.157,
            "range": "+/- 8991.37",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30688.41,
            "range": "+/- 177.445",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 793.755,
            "range": "+/- 3.426",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7980.901,
            "range": "+/- 64.936",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 339.878,
            "range": "+/- 1.413",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125556.475,
            "range": "+/- 1500.163",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7792.433,
            "range": "+/- 115.83",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8578.509,
            "range": "+/- 93.601",
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
          "id": "cf2e034de95d19e9fc8242dd41512799199af7b4",
          "message": "Merge pull request #144 from nervosys/perf/acpi-hw-reduced\n\nBoot MMIO guests on hardware-reduced ACPI: 600 fewer VM exits per boot",
          "timestamp": "2026-10-06T16:57:46-07:00",
          "tree_id": "b695c816b7e9e73a56e5279d38fdb34c9b1b1008",
          "url": "https://github.com/nervosys/HyperMachine/commit/cf2e034de95d19e9fc8242dd41512799199af7b4"
        },
        "date": 1791332042032,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3368.972,
            "range": "+/- 25.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 7121.885,
            "range": "+/- 34.552",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3061.635,
            "range": "+/- 13.517",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 3959.246,
            "range": "+/- 13.001",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2976.046,
            "range": "+/- 20.448",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 21149.749,
            "range": "+/- 210.022",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 3969.679,
            "range": "+/- 21.328",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 8286.855,
            "range": "+/- 46.629",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3699.572,
            "range": "+/- 14.927",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4695.536,
            "range": "+/- 16.008",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3683.451,
            "range": "+/- 20.053",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 20537.335,
            "range": "+/- 99.747",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 26535585.5,
            "range": "+/- 94638.668",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 688.708,
            "range": "+/- 6.003",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 688.877,
            "range": "+/- 2.311",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1058.026,
            "range": "+/- 6.241",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1927.12,
            "range": "+/- 15.865",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 438.678,
            "range": "+/- 1.745",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 639.101,
            "range": "+/- 2.805",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 727.467,
            "range": "+/- 4.074",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8557.635,
            "range": "+/- 27.067",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 316.49,
            "range": "+/- 1.232",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2316.554,
            "range": "+/- 11.556",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 218.156,
            "range": "+/- 1.373",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 46999.783,
            "range": "+/- 170.246",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8853.216,
            "range": "+/- 60.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 6882.098,
            "range": "+/- 14.684",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 646.935,
            "range": "+/- 3.965",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2056.67,
            "range": "+/- 7.23",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 654.816,
            "range": "+/- 4.298",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 26581.705,
            "range": "+/- 106.95",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 838.699,
            "range": "+/- 3.563",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1264.257,
            "range": "+/- 7.522",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11113.164,
            "range": "+/- 86.05",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 572.456,
            "range": "+/- 1.667",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 539183.945,
            "range": "+/- 2113.771",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8393.214,
            "range": "+/- 18.77",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 173.144,
            "range": "+/- 0.493",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2131.105,
            "range": "+/- 4.571",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 81.644,
            "range": "+/- 0.55",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 33842.945,
            "range": "+/- 179.889",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1825.165,
            "range": "+/- 11.171",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1594644.9,
            "range": "+/- 9966.346",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 24543.316,
            "range": "+/- 111.757",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 625.152,
            "range": "+/- 1.602",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6356.834,
            "range": "+/- 37.267",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 271.3,
            "range": "+/- 1.925",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 102053.081,
            "range": "+/- 883.898",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 4853.384,
            "range": "+/- 30.276",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5576.796,
            "range": "+/- 32.45",
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
          "id": "9785eae2ff15f1c83a83b6f678267bb6db922f9c",
          "message": "Merge pull request #145 from nervosys/docs/parity-boot-exits\n\nRecord the boot exit reductions in the parity matrix",
          "timestamp": "2026-10-06T18:54:17-07:00",
          "tree_id": "bb10556004ef74fc352615e68ee4606ff25f42ae",
          "url": "https://github.com/nervosys/HyperMachine/commit/9785eae2ff15f1c83a83b6f678267bb6db922f9c"
        },
        "date": 1791338955323,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3624.661,
            "range": "+/- 8.564",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8173.742,
            "range": "+/- 34.524",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3399.56,
            "range": "+/- 5.483",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4542.545,
            "range": "+/- 20.592",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3456.404,
            "range": "+/- 27.227",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 22675.088,
            "range": "+/- 41.789",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4588.937,
            "range": "+/- 25.783",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 10068.449,
            "range": "+/- 119.991",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4260.399,
            "range": "+/- 14.117",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 5879.528,
            "range": "+/- 75.722",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4204.462,
            "range": "+/- 32.202",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 25703.949,
            "range": "+/- 328.543",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 36874586,
            "range": "+/- 88054.994",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 760.735,
            "range": "+/- 2.683",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 768.244,
            "range": "+/- 1.912",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1229.587,
            "range": "+/- 2.158",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2211.664,
            "range": "+/- 6.971",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 541.243,
            "range": "+/- 5.81",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 755.927,
            "range": "+/- 0.967",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 790.819,
            "range": "+/- 0.708",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9284.325,
            "range": "+/- 4.465",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 368.655,
            "range": "+/- 0.963",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2489.634,
            "range": "+/- 1.638",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 258.826,
            "range": "+/- 0.309",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59600.954,
            "range": "+/- 252.56",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8919.326,
            "range": "+/- 86.088",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7857.516,
            "range": "+/- 10.602",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 716.073,
            "range": "+/- 1.249",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2322.146,
            "range": "+/- 1.295",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 715.23,
            "range": "+/- 1.402",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 30009.365,
            "range": "+/- 58.694",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 953.748,
            "range": "+/- 3.78",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1557.252,
            "range": "+/- 5.396",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12272.88,
            "range": "+/- 99.34",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 632.541,
            "range": "+/- 0.385",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 580536.105,
            "range": "+/- 220.678",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9171.021,
            "range": "+/- 9.808",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 207.638,
            "range": "+/- 0.148",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2335.781,
            "range": "+/- 2.523",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 102.276,
            "range": "+/- 0.128",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36342.216,
            "range": "+/- 12.119",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1914.946,
            "range": "+/- 18.114",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1644537.698,
            "range": "+/- 4403.808",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 25915.279,
            "range": "+/- 63.178",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 691.044,
            "range": "+/- 3.984",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6775.807,
            "range": "+/- 40.382",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 300.462,
            "range": "+/- 0.665",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 104740.342,
            "range": "+/- 1229.419",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5214.785,
            "range": "+/- 24.89",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5890.513,
            "range": "+/- 34.917",
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
          "id": "4748e0cee8e05e497465c210b4527d8ddf044761",
          "message": "Merge pull request #146 from nervosys/feat/block-volumes\n\nAdd movable block disks: one sandbox at a time, data kept between them",
          "timestamp": "2026-10-07T06:27:55-07:00",
          "tree_id": "2a1ca38a6a742180f3f5dc8bf5ec66c528c3ced9",
          "url": "https://github.com/nervosys/HyperMachine/commit/4748e0cee8e05e497465c210b4527d8ddf044761"
        },
        "date": 1791380579166,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3634.962,
            "range": "+/- 8.553",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8236.438,
            "range": "+/- 68.91",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3400.457,
            "range": "+/- 9.445",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4502.408,
            "range": "+/- 12.439",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3324.879,
            "range": "+/- 4.458",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 22918.07,
            "range": "+/- 94.193",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4490.338,
            "range": "+/- 11.329",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 9527.849,
            "range": "+/- 26.08",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4250.81,
            "range": "+/- 6.974",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 5402.594,
            "range": "+/- 10.007",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4184.204,
            "range": "+/- 14.66",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 28382.21,
            "range": "+/- 121.219",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 36881450,
            "range": "+/- 84440.206",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 796.81,
            "range": "+/- 1.582",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 802.278,
            "range": "+/- 1.105",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1369.905,
            "range": "+/- 7.482",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2301.766,
            "range": "+/- 2.844",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 586.702,
            "range": "+/- 2.666",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 864.318,
            "range": "+/- 5.679",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 792.33,
            "range": "+/- 0.728",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9293.108,
            "range": "+/- 8.518",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 370.637,
            "range": "+/- 2.036",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2501.802,
            "range": "+/- 3.431",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 259.676,
            "range": "+/- 0.894",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59306.871,
            "range": "+/- 178.277",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8642.903,
            "range": "+/- 61.6",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7806.248,
            "range": "+/- 13.882",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 721.335,
            "range": "+/- 1.182",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2331.904,
            "range": "+/- 4.933",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 727.233,
            "range": "+/- 1.8",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 29981.164,
            "range": "+/- 80.749",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 954.671,
            "range": "+/- 1.901",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1572.705,
            "range": "+/- 3.922",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11881.979,
            "range": "+/- 48.579",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 633.105,
            "range": "+/- 0.355",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 581383.776,
            "range": "+/- 447.222",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9138.879,
            "range": "+/- 14.942",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 209.666,
            "range": "+/- 0.673",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2342.633,
            "range": "+/- 2.372",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 102.233,
            "range": "+/- 0.224",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36406.187,
            "range": "+/- 59.209",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1875.088,
            "range": "+/- 3.395",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1652299.111,
            "range": "+/- 5296.685",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 25948.2,
            "range": "+/- 44.655",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 679.491,
            "range": "+/- 3.403",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6713.17,
            "range": "+/- 23.183",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 295.334,
            "range": "+/- 0.657",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 103381.393,
            "range": "+/- 285.115",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5278.948,
            "range": "+/- 29.3",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5835.039,
            "range": "+/- 19.638",
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
          "id": "13aaa38404ed3736e2d15b7a9d3af8e95b129a9c",
          "message": "Merge pull request #147 from nervosys/feat/reboot-in-place\n\nReboot sandboxes in place, and make a guest's reboot reach the host",
          "timestamp": "2026-10-07T08:24:33-07:00",
          "tree_id": "ee0ef74761979f20c006baf429e75f9b47f9dd60",
          "url": "https://github.com/nervosys/HyperMachine/commit/13aaa38404ed3736e2d15b7a9d3af8e95b129a9c"
        },
        "date": 1791387595633,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 2736.149,
            "range": "+/- 9.883",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 6413.666,
            "range": "+/- 12.507",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 2544.654,
            "range": "+/- 7.12",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 3467.898,
            "range": "+/- 5.759",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2463.922,
            "range": "+/- 4.014",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 18254.317,
            "range": "+/- 33.017",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 3567.784,
            "range": "+/- 9.637",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 7605.099,
            "range": "+/- 28.902",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3499.606,
            "range": "+/- 13.417",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4352.686,
            "range": "+/- 29.263",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3380.777,
            "range": "+/- 29.513",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 19706.391,
            "range": "+/- 182.408",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 27264768,
            "range": "+/- 169800.69",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 853.834,
            "range": "+/- 1.222",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 856.087,
            "range": "+/- 1.461",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1133.106,
            "range": "+/- 2.661",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1976.222,
            "range": "+/- 10.975",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 456.381,
            "range": "+/- 2.892",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 719.376,
            "range": "+/- 2.399",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 696.415,
            "range": "+/- 2.047",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8261.695,
            "range": "+/- 22.74",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 325.583,
            "range": "+/- 0.593",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2166.081,
            "range": "+/- 6.743",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 232.623,
            "range": "+/- 1.173",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 44526.5,
            "range": "+/- 172.647",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 6384.808,
            "range": "+/- 40.617",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7806.879,
            "range": "+/- 14.634",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 801.024,
            "range": "+/- 3.502",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2433.917,
            "range": "+/- 16.872",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 800.993,
            "range": "+/- 3.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 29112.215,
            "range": "+/- 50.063",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1033.18,
            "range": "+/- 1.578",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1169.699,
            "range": "+/- 4.695",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 8136.764,
            "range": "+/- 60.184",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 542.472,
            "range": "+/- 2.723",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 507744.231,
            "range": "+/- 3582.082",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8005.68,
            "range": "+/- 16.821",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 164.723,
            "range": "+/- 0.418",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2060.852,
            "range": "+/- 7.434",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 71.257,
            "range": "+/- 0.229",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 31563.224,
            "range": "+/- 130.626",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1378.597,
            "range": "+/- 7.144",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1189854.54,
            "range": "+/- 4540.05",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 19033.78,
            "range": "+/- 45.591",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 492.65,
            "range": "+/- 1.358",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 5040.455,
            "range": "+/- 22.827",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 214.648,
            "range": "+/- 1.271",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 75998.63,
            "range": "+/- 373.382",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 3876.509,
            "range": "+/- 41.233",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 4253.008,
            "range": "+/- 22.128",
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
          "id": "9177a659a63a3fb162166a80f33ecafbcd2a2a4b",
          "message": "Merge pull request #149 from nervosys/feat/team-isolation\n\nIsolate tenants by team on the control plane",
          "timestamp": "2026-10-07T10:06:29-07:00",
          "tree_id": "7483837673ffde52f4d3b621a2a858aa02ec30bd",
          "url": "https://github.com/nervosys/HyperMachine/commit/9177a659a63a3fb162166a80f33ecafbcd2a2a4b"
        },
        "date": 1791393865039,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 2843.801,
            "range": "+/- 60.794",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 6339.242,
            "range": "+/- 47.764",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 2466.147,
            "range": "+/- 12.84",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 3600.071,
            "range": "+/- 35.214",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2330.134,
            "range": "+/- 4.222",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 18445.699,
            "range": "+/- 153.44",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 3409.129,
            "range": "+/- 11.72",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 7582.105,
            "range": "+/- 73.257",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3243.92,
            "range": "+/- 6.205",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4222.079,
            "range": "+/- 13.793",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3197.972,
            "range": "+/- 11.952",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 19815.759,
            "range": "+/- 110.644",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 29065479.5,
            "range": "+/- 295568.728",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 768.291,
            "range": "+/- 2.09",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 779.078,
            "range": "+/- 3.221",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 987.64,
            "range": "+/- 2.419",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1763.808,
            "range": "+/- 5.159",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 419.486,
            "range": "+/- 1.65",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 616.516,
            "range": "+/- 2.029",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 684.142,
            "range": "+/- 2.209",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 7792.801,
            "range": "+/- 49.903",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 325.077,
            "range": "+/- 2.647",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2116.82,
            "range": "+/- 4.94",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 235.142,
            "range": "+/- 0.981",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 46437.994,
            "range": "+/- 585.966",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 6093.262,
            "range": "+/- 33.449",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7081.184,
            "range": "+/- 52.432",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 702.747,
            "range": "+/- 1.776",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2144.702,
            "range": "+/- 8.841",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 701.151,
            "range": "+/- 1.408",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 26091.169,
            "range": "+/- 82.594",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 906.441,
            "range": "+/- 4.374",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1167.847,
            "range": "+/- 14.97",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 8419.68,
            "range": "+/- 112.932",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 517.331,
            "range": "+/- 3.009",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 490848.898,
            "range": "+/- 1652.816",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 7553.341,
            "range": "+/- 13.122",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 163.636,
            "range": "+/- 1.176",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 1952.32,
            "range": "+/- 5.681",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 69.586,
            "range": "+/- 0.378",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 30069.425,
            "range": "+/- 60.471",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1487.816,
            "range": "+/- 15.272",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1192165.42,
            "range": "+/- 5417.121",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 19180.016,
            "range": "+/- 100.207",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 495.693,
            "range": "+/- 3.491",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 5069.41,
            "range": "+/- 60.719",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 210.254,
            "range": "+/- 2.304",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 75784.433,
            "range": "+/- 268.605",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 3944.493,
            "range": "+/- 44.374",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 4375.974,
            "range": "+/- 46.13",
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
          "id": "f68253263df549db201f2ef7bf5f7aceffb0464b",
          "message": "Merge pull request #150 from nervosys/feat/team-events\n\nPartition events and webhooks by team",
          "timestamp": "2026-10-07T10:49:53-07:00",
          "tree_id": "dff5536bee42f4c0fd5a71974771f0f827fde384",
          "url": "https://github.com/nervosys/HyperMachine/commit/f68253263df549db201f2ef7bf5f7aceffb0464b"
        },
        "date": 1791396450658,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4282.293,
            "range": "+/- 12.076",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9581.895,
            "range": "+/- 25.702",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4294.13,
            "range": "+/- 59.472",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5283.783,
            "range": "+/- 13.219",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3925.21,
            "range": "+/- 7.711",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27090.09,
            "range": "+/- 118.116",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5272.942,
            "range": "+/- 38.065",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11196.154,
            "range": "+/- 41.475",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5153.325,
            "range": "+/- 53.636",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6334.778,
            "range": "+/- 20.646",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 5585.963,
            "range": "+/- 276.274",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29499.995,
            "range": "+/- 89.137",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44851449.5,
            "range": "+/- 258524.482",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 901.672,
            "range": "+/- 1.741",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 920.387,
            "range": "+/- 5.182",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1432.38,
            "range": "+/- 7.688",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2637.977,
            "range": "+/- 15.466",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 594.528,
            "range": "+/- 2.255",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 875.57,
            "range": "+/- 3.235",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 906.643,
            "range": "+/- 1.117",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10597.356,
            "range": "+/- 5.78",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 427.426,
            "range": "+/- 2.829",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2994.215,
            "range": "+/- 18.462",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 301.962,
            "range": "+/- 1.395",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75299.91,
            "range": "+/- 818.018",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13624.809,
            "range": "+/- 174.642",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9179.367,
            "range": "+/- 47.549",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 834.611,
            "range": "+/- 5.127",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2677.904,
            "range": "+/- 9.991",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 840.288,
            "range": "+/- 5.852",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35009.696,
            "range": "+/- 187.711",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1084.555,
            "range": "+/- 4.636",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2070.961,
            "range": "+/- 32.919",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16841.532,
            "range": "+/- 164.657",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 727.179,
            "range": "+/- 0.948",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662523.829,
            "range": "+/- 219.035",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10400.57,
            "range": "+/- 4.715",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.137,
            "range": "+/- 0.333",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2671.906,
            "range": "+/- 2.284",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.288,
            "range": "+/- 0.173",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41504.142,
            "range": "+/- 46.823",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2238.813,
            "range": "+/- 15.865",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1978191.042,
            "range": "+/- 14005.175",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30615.522,
            "range": "+/- 176.645",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 815.166,
            "range": "+/- 6.954",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8214.258,
            "range": "+/- 133.256",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 340.257,
            "range": "+/- 2.449",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 121431.096,
            "range": "+/- 553.155",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7575.908,
            "range": "+/- 86.491",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8494.871,
            "range": "+/- 89.061",
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
          "id": "5a89d46bb2321f8e1edfcfe2258ce754eeee162d",
          "message": "Merge pull request #151 from nervosys/feat/team-volumes\n\nPartition volumes by team",
          "timestamp": "2026-10-07T11:45:17-07:00",
          "tree_id": "368a7a2c517b88ab18c12b70de06fae8ed4c3b7a",
          "url": "https://github.com/nervosys/HyperMachine/commit/5a89d46bb2321f8e1edfcfe2258ce754eeee162d"
        },
        "date": 1791399715130,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 3250.699,
            "range": "+/- 10.085",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 7223.813,
            "range": "+/- 43.802",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3084.54,
            "range": "+/- 14.929",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4025.56,
            "range": "+/- 13.059",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2977.378,
            "range": "+/- 8.659",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 20455.718,
            "range": "+/- 49.968",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 4027.739,
            "range": "+/- 21.56",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 8426.162,
            "range": "+/- 20.152",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3807.62,
            "range": "+/- 15.522",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4870.857,
            "range": "+/- 15.533",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3694.662,
            "range": "+/- 5.588",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 22510.677,
            "range": "+/- 122.104",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 26276966,
            "range": "+/- 88327.475",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 698.784,
            "range": "+/- 2.026",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 706.91,
            "range": "+/- 2.757",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1212.323,
            "range": "+/- 5.727",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1978.394,
            "range": "+/- 8.512",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 452.577,
            "range": "+/- 2.5",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 658.519,
            "range": "+/- 2.736",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 762.388,
            "range": "+/- 14.079",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8521.105,
            "range": "+/- 11.193",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 319.618,
            "range": "+/- 1.407",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2263.239,
            "range": "+/- 2.16",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 223.707,
            "range": "+/- 1.147",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 49791.778,
            "range": "+/- 167.699",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8782.198,
            "range": "+/- 33.695",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7170.575,
            "range": "+/- 22.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 663.401,
            "range": "+/- 2.858",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2176.142,
            "range": "+/- 13.094",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 653.27,
            "range": "+/- 3.154",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 27479.11,
            "range": "+/- 75.491",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 860.525,
            "range": "+/- 3.614",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1258.419,
            "range": "+/- 7.485",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11570.235,
            "range": "+/- 106.079",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 590.942,
            "range": "+/- 2.515",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 560382.655,
            "range": "+/- 2553.965",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8588.305,
            "range": "+/- 31.039",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 185.235,
            "range": "+/- 0.901",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2214.023,
            "range": "+/- 8.424",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 79.647,
            "range": "+/- 0.247",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 33929.837,
            "range": "+/- 127.836",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1805.617,
            "range": "+/- 10.403",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1608309.779,
            "range": "+/- 11651.903",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 24780.146,
            "range": "+/- 139.934",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 662.358,
            "range": "+/- 4.492",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6414.248,
            "range": "+/- 42.813",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 269.622,
            "range": "+/- 1.588",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 102574.752,
            "range": "+/- 985.031",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5154.998,
            "range": "+/- 39.364",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5346.897,
            "range": "+/- 16.545",
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
          "id": "5f9c4d0468e66a7a0b116c8f440541a1031278a3",
          "message": "Merge pull request #155 from nervosys/feat/sso-login\n\nSingle sign-on through any OpenID Connect provider (browser and CLI)",
          "timestamp": "2026-10-07T14:02:32-07:00",
          "tree_id": "a0ee3e1129e2b1f5604cc33ad640fb52a5a5b836",
          "url": "https://github.com/nervosys/HyperMachine/commit/5f9c4d0468e66a7a0b116c8f440541a1031278a3"
        },
        "date": 1791407845599,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4701.714,
            "range": "+/- 14.809",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 10523.495,
            "range": "+/- 34.382",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4413.426,
            "range": "+/- 12.459",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5803.736,
            "range": "+/- 12.81",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4451.568,
            "range": "+/- 43.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 30067.284,
            "range": "+/- 234.407",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5851.251,
            "range": "+/- 22.411",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12323.297,
            "range": "+/- 38.572",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5511.413,
            "range": "+/- 13.013",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 7032.392,
            "range": "+/- 20.822",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 5436.141,
            "range": "+/- 19.299",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 30860.342,
            "range": "+/- 220.776",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 47604703,
            "range": "+/- 70045.07",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 1015.306,
            "range": "+/- 2.145",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 1031.006,
            "range": "+/- 3.184",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1731.985,
            "range": "+/- 10.739",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2970.282,
            "range": "+/- 26.71",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 739.003,
            "range": "+/- 5.703",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 1065.482,
            "range": "+/- 7",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1019.406,
            "range": "+/- 0.698",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12075.035,
            "range": "+/- 12.943",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 472.59,
            "range": "+/- 0.654",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3240.285,
            "range": "+/- 6.583",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 333.676,
            "range": "+/- 0.622",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 83707.422,
            "range": "+/- 557.337",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11264.704,
            "range": "+/- 87.857",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 10030.634,
            "range": "+/- 14.791",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 912.796,
            "range": "+/- 2.686",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2969.103,
            "range": "+/- 3.669",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 908.713,
            "range": "+/- 1.946",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 38489.903,
            "range": "+/- 60.706",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1310.44,
            "range": "+/- 17.224",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2200.04,
            "range": "+/- 9.229",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16142.861,
            "range": "+/- 224.931",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 818.573,
            "range": "+/- 0.794",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 749095.739,
            "range": "+/- 383.741",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11794.762,
            "range": "+/- 3.794",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 270.594,
            "range": "+/- 0.819",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3015.194,
            "range": "+/- 1.457",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 151.472,
            "range": "+/- 2.389",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46900.39,
            "range": "+/- 70.631",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2425.376,
            "range": "+/- 7.44",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2138613.958,
            "range": "+/- 7987.029",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33790.59,
            "range": "+/- 139.15",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 869.45,
            "range": "+/- 3.742",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8685.482,
            "range": "+/- 43.922",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 393.818,
            "range": "+/- 1.066",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 138368.803,
            "range": "+/- 1173.985",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6821.626,
            "range": "+/- 25.685",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7658.033,
            "range": "+/- 57.185",
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
          "id": "ae0d561afe66a781f028a4e43edf2330238ced5a",
          "message": "Merge pull request #153 from nervosys/test/teams-kvm\n\nVerify team isolation on a real KVM cluster with two teams",
          "timestamp": "2026-10-08T10:25:42-07:00",
          "tree_id": "fa6d05c7a0b3c6fd7ccc3e3e9c171d04bd37746a",
          "url": "https://github.com/nervosys/HyperMachine/commit/ae0d561afe66a781f028a4e43edf2330238ced5a"
        },
        "date": 1791481680314,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4264.664,
            "range": "+/- 15.31",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9597.053,
            "range": "+/- 38.959",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3979.809,
            "range": "+/- 5.49",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5276.055,
            "range": "+/- 12.342",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3896.72,
            "range": "+/- 4.841",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26741.8,
            "range": "+/- 81.025",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5220.19,
            "range": "+/- 17.175",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12439.964,
            "range": "+/- 57.767",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4939.969,
            "range": "+/- 15.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6341.163,
            "range": "+/- 21.474",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4858.858,
            "range": "+/- 20.339",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 33963.095,
            "range": "+/- 141.517",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43334374.5,
            "range": "+/- 84042.96",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 916.91,
            "range": "+/- 3.743",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 939.326,
            "range": "+/- 7.083",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1480.559,
            "range": "+/- 10.806",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2642.192,
            "range": "+/- 18.203",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 596.411,
            "range": "+/- 1.618",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 921.549,
            "range": "+/- 8.699",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 907.531,
            "range": "+/- 1.14",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10646.205,
            "range": "+/- 27.973",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 437.989,
            "range": "+/- 3.285",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2853.268,
            "range": "+/- 4.348",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 304.482,
            "range": "+/- 1.683",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72952.274,
            "range": "+/- 486.6",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12443.178,
            "range": "+/- 52.613",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9257.997,
            "range": "+/- 70.777",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 868.256,
            "range": "+/- 6.716",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2658.479,
            "range": "+/- 6.213",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 864.455,
            "range": "+/- 4.721",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 35461.188,
            "range": "+/- 489.663",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1094.984,
            "range": "+/- 4.909",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1918.71,
            "range": "+/- 15.368",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17125.805,
            "range": "+/- 167.453",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 768.305,
            "range": "+/- 8.013",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662503.549,
            "range": "+/- 507.471",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10442.671,
            "range": "+/- 8.062",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.792,
            "range": "+/- 1.316",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2668.073,
            "range": "+/- 4.122",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.057,
            "range": "+/- 0.192",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41513.252,
            "range": "+/- 25.529",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2225.62,
            "range": "+/- 15.495",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1930483.002,
            "range": "+/- 8226.554",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30596.599,
            "range": "+/- 180.677",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 806.01,
            "range": "+/- 4.999",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7886.903,
            "range": "+/- 33.383",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 348.359,
            "range": "+/- 1.857",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 120806.461,
            "range": "+/- 539.438",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7306.58,
            "range": "+/- 82.413",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8382.353,
            "range": "+/- 74.087",
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
          "id": "5ac42a44bf95b2c149f6c9d2c52493a7479d61a9",
          "message": "Merge pull request #159 from nervosys/feat/fips-tls\n\nAdd a FIPS build: TLS on AWS-LC's validated module",
          "timestamp": "2026-10-08T11:41:04-07:00",
          "tree_id": "95dd0fd7c9533cadbf5da57ea9d171a8dc2ced60",
          "url": "https://github.com/nervosys/HyperMachine/commit/5ac42a44bf95b2c149f6c9d2c52493a7479d61a9"
        },
        "date": 1791485880613,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4265.759,
            "range": "+/- 12.357",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9738.525,
            "range": "+/- 76.076",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4002.723,
            "range": "+/- 10.903",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5304.076,
            "range": "+/- 22.062",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3911.403,
            "range": "+/- 12.623",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27134.74,
            "range": "+/- 135.049",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5206.574,
            "range": "+/- 18.313",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13443.665,
            "range": "+/- 52.905",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4915.632,
            "range": "+/- 15.442",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6338.097,
            "range": "+/- 31.871",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4914.753,
            "range": "+/- 15.569",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29288.306,
            "range": "+/- 79.682",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43196526,
            "range": "+/- 113706.814",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 925.625,
            "range": "+/- 6.607",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 921.683,
            "range": "+/- 2.17",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1435.394,
            "range": "+/- 4.356",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2541.75,
            "range": "+/- 9.246",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 605.534,
            "range": "+/- 2.33",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 879.629,
            "range": "+/- 2.709",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 921.297,
            "range": "+/- 1.082",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10670.521,
            "range": "+/- 19.366",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 436.917,
            "range": "+/- 1.077",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2862.461,
            "range": "+/- 2.842",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 315.457,
            "range": "+/- 0.863",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76063.796,
            "range": "+/- 870.06",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13287.549,
            "range": "+/- 205.038",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9735.977,
            "range": "+/- 35.353",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 838.602,
            "range": "+/- 1.957",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2876.671,
            "range": "+/- 5.543",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 843.273,
            "range": "+/- 4.495",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 37013.818,
            "range": "+/- 146.787",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1130.017,
            "range": "+/- 5.152",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2080.754,
            "range": "+/- 24.756",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17087.204,
            "range": "+/- 258.384",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 725.416,
            "range": "+/- 0.52",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664646.578,
            "range": "+/- 558.765",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10493.919,
            "range": "+/- 15.063",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.001,
            "range": "+/- 0.297",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2664.558,
            "range": "+/- 1.341",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.425,
            "range": "+/- 0.161",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41523.362,
            "range": "+/- 29.265",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2226.281,
            "range": "+/- 23.005",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2022631.8,
            "range": "+/- 25567.837",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30129.327,
            "range": "+/- 70.862",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 801.847,
            "range": "+/- 6.295",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8340.805,
            "range": "+/- 120.484",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 337.997,
            "range": "+/- 1.349",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 119851.321,
            "range": "+/- 336.042",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7172.251,
            "range": "+/- 49.736",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8067.213,
            "range": "+/- 50.298",
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
          "id": "03f1fca83730559070290041fe4f0b63cb3f5ec4",
          "message": "Merge pull request #161 from nervosys/feat/machine-network\n\nGive machines a network: one NIC behind the node's egress gateway",
          "timestamp": "2026-10-08T12:55:14-07:00",
          "tree_id": "cdc22a3a1025aab58e5a2e0fbfc63833adf05a59",
          "url": "https://github.com/nervosys/HyperMachine/commit/03f1fca83730559070290041fe4f0b63cb3f5ec4"
        },
        "date": 1791490398383,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4244.207,
            "range": "+/- 9.512",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9603.071,
            "range": "+/- 40.414",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4002.529,
            "range": "+/- 14.574",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5282.809,
            "range": "+/- 17.81",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4089.782,
            "range": "+/- 57.504",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 57326.342,
            "range": "+/- 175.017",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5235.05,
            "range": "+/- 15.15",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 14132.986,
            "range": "+/- 200.667",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4993.852,
            "range": "+/- 25.308",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6357.207,
            "range": "+/- 28.327",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4926.806,
            "range": "+/- 40.436",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29753.743,
            "range": "+/- 215.247",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45926074,
            "range": "+/- 756818.917",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 924.078,
            "range": "+/- 3.276",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 949.94,
            "range": "+/- 7.731",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1430.179,
            "range": "+/- 4.264",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2562.626,
            "range": "+/- 12.361",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 607.603,
            "range": "+/- 2.025",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 882.621,
            "range": "+/- 2.451",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 906.96,
            "range": "+/- 1.339",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10624.449,
            "range": "+/- 9.645",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 421.364,
            "range": "+/- 0.743",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2861.869,
            "range": "+/- 6.29",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 300.889,
            "range": "+/- 0.732",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75715.073,
            "range": "+/- 1111.823",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 14250.898,
            "range": "+/- 465.078",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9342.87,
            "range": "+/- 94.625",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 853.913,
            "range": "+/- 4.824",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2755.739,
            "range": "+/- 17.239",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 888.265,
            "range": "+/- 6.494",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34111.923,
            "range": "+/- 81.941",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1147.984,
            "range": "+/- 8.674",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1956.841,
            "range": "+/- 10.083",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16347.661,
            "range": "+/- 68.348",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 723.944,
            "range": "+/- 0.588",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 665693.409,
            "range": "+/- 1012.311",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10403.277,
            "range": "+/- 7.514",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 239.993,
            "range": "+/- 0.204",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2663.377,
            "range": "+/- 2.899",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.496,
            "range": "+/- 0.337",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41395.445,
            "range": "+/- 28.189",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2205.907,
            "range": "+/- 7.45",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1942745.685,
            "range": "+/- 10523.149",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30619.624,
            "range": "+/- 264.747",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 791.783,
            "range": "+/- 2.144",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7862.878,
            "range": "+/- 25.552",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 339.834,
            "range": "+/- 1.083",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 120907.714,
            "range": "+/- 550.6",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7266.785,
            "range": "+/- 53.543",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8094.734,
            "range": "+/- 32.869",
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
          "id": "4185b85d8a84471cd7c7ab97c511a81a71b7b63c",
          "message": "Merge pull request #164 from nervosys/feat/machines-control-plane\n\nServe machines through the control plane, each team's its own",
          "timestamp": "2026-10-08T13:16:02-07:00",
          "tree_id": "8d8fed811823c17121de0bbaed681f86a9656ba2",
          "url": "https://github.com/nervosys/HyperMachine/commit/4185b85d8a84471cd7c7ab97c511a81a71b7b63c"
        },
        "date": 1791491905905,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4042.168,
            "range": "+/- 17.756",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 8411.84,
            "range": "+/- 51.528",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3807.684,
            "range": "+/- 12.362",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 4903.894,
            "range": "+/- 24.122",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3771.229,
            "range": "+/- 20.46",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 23494.478,
            "range": "+/- 108.576",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5181.615,
            "range": "+/- 16.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 10165.953,
            "range": "+/- 39.893",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4949.728,
            "range": "+/- 25.594",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6096.653,
            "range": "+/- 13.579",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4922.21,
            "range": "+/- 29.175",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 24483.38,
            "range": "+/- 112.668",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 37819462.5,
            "range": "+/- 98002.066",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 1030.864,
            "range": "+/- 3.703",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 1040.922,
            "range": "+/- 4.275",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1762.39,
            "range": "+/- 21.296",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2916.908,
            "range": "+/- 21.694",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 670.419,
            "range": "+/- 2.82",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 981.575,
            "range": "+/- 5.047",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1101.282,
            "range": "+/- 5.181",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12904.893,
            "range": "+/- 37.042",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 499.205,
            "range": "+/- 2.116",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3462.102,
            "range": "+/- 11.368",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 350.459,
            "range": "+/- 1.551",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76909.294,
            "range": "+/- 1694.968",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11162.776,
            "range": "+/- 110.725",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 10292.685,
            "range": "+/- 38.677",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 947.062,
            "range": "+/- 4.404",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 3078.203,
            "range": "+/- 13.949",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 943.662,
            "range": "+/- 3.217",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 41401.507,
            "range": "+/- 465.759",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1399.385,
            "range": "+/- 21.191",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2030.308,
            "range": "+/- 23.821",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 14399.682,
            "range": "+/- 152.995",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 864.914,
            "range": "+/- 2.643",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 811182.49,
            "range": "+/- 2717.84",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 12703.25,
            "range": "+/- 53.828",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 273.92,
            "range": "+/- 1.003",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3250.094,
            "range": "+/- 12.896",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 134.713,
            "range": "+/- 1.599",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 51318.724,
            "range": "+/- 219.955",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2565.946,
            "range": "+/- 13.803",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2281908.955,
            "range": "+/- 10291.074",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35839.988,
            "range": "+/- 159.596",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 894.72,
            "range": "+/- 3.209",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9274.402,
            "range": "+/- 58.711",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 401.29,
            "range": "+/- 21.592",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143027.173,
            "range": "+/- 556.241",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6750.327,
            "range": "+/- 86.55",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7206.184,
            "range": "+/- 49.344",
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
          "id": "53b9866aee5d2994ffee454a1fb4bb86b3df1387",
          "message": "Merge pull request #165 from nervosys/docs/vmware-plan-control-plane\n\nTest machine placement across nodes, and update the VMware plan's status",
          "timestamp": "2026-10-08T14:58:33-07:00",
          "tree_id": "19982417ba44a5f78595f99bcb7bd8c64d6b13c8",
          "url": "https://github.com/nervosys/HyperMachine/commit/53b9866aee5d2994ffee454a1fb4bb86b3df1387"
        },
        "date": 1791497870635,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4348.632,
            "range": "+/- 32.904",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9598.848,
            "range": "+/- 44.502",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4078.285,
            "range": "+/- 20.911",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5349.74,
            "range": "+/- 23.753",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4004.781,
            "range": "+/- 27.131",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27051.459,
            "range": "+/- 94.674",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5246.617,
            "range": "+/- 20.079",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12203.275,
            "range": "+/- 72.585",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4989.85,
            "range": "+/- 16.928",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6324.427,
            "range": "+/- 19.207",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4871.305,
            "range": "+/- 13.017",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29680.598,
            "range": "+/- 95.323",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43556448.5,
            "range": "+/- 137832.264",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 1002.368,
            "range": "+/- 9.414",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 983.254,
            "range": "+/- 2.221",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1426.253,
            "range": "+/- 3.175",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2580.844,
            "range": "+/- 14.021",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 604.711,
            "range": "+/- 4.113",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 875.781,
            "range": "+/- 2.609",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 911.033,
            "range": "+/- 1.442",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10633.989,
            "range": "+/- 19.662",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 423.568,
            "range": "+/- 0.771",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2849.718,
            "range": "+/- 2.184",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 303.383,
            "range": "+/- 0.804",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73620.078,
            "range": "+/- 482.4",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12460.238,
            "range": "+/- 67.965",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9048.846,
            "range": "+/- 28.264",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 828.179,
            "range": "+/- 2.048",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2684.455,
            "range": "+/- 6.615",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 832.645,
            "range": "+/- 3.208",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34580.672,
            "range": "+/- 120.785",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1096.023,
            "range": "+/- 2.835",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2162.514,
            "range": "+/- 43.912",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16451.555,
            "range": "+/- 73.016",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 727.196,
            "range": "+/- 0.573",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 668322.476,
            "range": "+/- 629.145",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10464.26,
            "range": "+/- 10.852",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.148,
            "range": "+/- 0.352",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2688.131,
            "range": "+/- 3.778",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.455,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41808.888,
            "range": "+/- 90.968",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2204.906,
            "range": "+/- 6.918",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1982026.922,
            "range": "+/- 22887.769",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30626.314,
            "range": "+/- 155.08",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 945.557,
            "range": "+/- 18.624",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7908.555,
            "range": "+/- 52.087",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 402.826,
            "range": "+/- 7.494",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 123423.014,
            "range": "+/- 951.428",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7352.478,
            "range": "+/- 63.219",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8599.129,
            "range": "+/- 114.109",
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
          "id": "b177fefed50218b68b62cd2e8ddb768e03a515d1",
          "message": "Merge pull request #166 from nervosys/feat/hm-machines\n\nAdd hm sandbox vm machine: machines from the CLI",
          "timestamp": "2026-10-08T15:31:36-07:00",
          "tree_id": "34381269a1de5dcdab29c4e454d51b77b09fe650",
          "url": "https://github.com/nervosys/HyperMachine/commit/b177fefed50218b68b62cd2e8ddb768e03a515d1"
        },
        "date": 1791499785973,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4391.464,
            "range": "+/- 15.922",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9748.241,
            "range": "+/- 52.478",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4196.805,
            "range": "+/- 15.338",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5426.704,
            "range": "+/- 15.1",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4565.039,
            "range": "+/- 170.762",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27529.225,
            "range": "+/- 139.145",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5369.556,
            "range": "+/- 14.84",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11333.418,
            "range": "+/- 34.292",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5048.014,
            "range": "+/- 11.856",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6414.989,
            "range": "+/- 15.268",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 5023.068,
            "range": "+/- 11.996",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 30542.632,
            "range": "+/- 372.257",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 35760809,
            "range": "+/- 281006.698",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 926.115,
            "range": "+/- 2.167",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 935.265,
            "range": "+/- 3.854",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1397.562,
            "range": "+/- 4.539",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2472.109,
            "range": "+/- 9.556",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 582.868,
            "range": "+/- 1.057",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 861.973,
            "range": "+/- 4.805",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 955.613,
            "range": "+/- 3.39",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11510.005,
            "range": "+/- 19.952",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 420.903,
            "range": "+/- 1.232",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3090.14,
            "range": "+/- 11.285",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 289.886,
            "range": "+/- 0.918",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 68568.534,
            "range": "+/- 518.698",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11727.225,
            "range": "+/- 148.79",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9389.442,
            "range": "+/- 34.837",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 839.859,
            "range": "+/- 2.214",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2763.62,
            "range": "+/- 6.496",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 846.983,
            "range": "+/- 2.816",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 37597.594,
            "range": "+/- 714.883",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1118.603,
            "range": "+/- 4.121",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2078.28,
            "range": "+/- 33.268",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15438.649,
            "range": "+/- 125.395",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 776.82,
            "range": "+/- 2.884",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 728605.588,
            "range": "+/- 2602.78",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11603.413,
            "range": "+/- 78.278",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 243.827,
            "range": "+/- 0.703",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2882.563,
            "range": "+/- 8.44",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 107.479,
            "range": "+/- 0.39",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 45771.514,
            "range": "+/- 141.1",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2775.112,
            "range": "+/- 65.071",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2099156.708,
            "range": "+/- 7802.219",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33433.112,
            "range": "+/- 102.207",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 842.542,
            "range": "+/- 1.569",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8599.762,
            "range": "+/- 33.367",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 346.937,
            "range": "+/- 1.289",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 133140.703,
            "range": "+/- 650.715",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6714.052,
            "range": "+/- 55.502",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7266.783,
            "range": "+/- 28.892",
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
          "id": "0a1ae66c416e8cb87b6f7e3336b878cfcefb8fd9",
          "message": "Merge pull request #167 from nervosys/feat/pvh-firmware-boot\n\nBoot firmware by PVH, with a disk on the PCI bus",
          "timestamp": "2026-10-08T16:15:04-07:00",
          "tree_id": "5d71787b6453acf8d03a48d9a1b28174e5c78616",
          "url": "https://github.com/nervosys/HyperMachine/commit/0a1ae66c416e8cb87b6f7e3336b878cfcefb8fd9"
        },
        "date": 1791502147822,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 2716.768,
            "range": "+/- 11.93",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 6632.897,
            "range": "+/- 46.647",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 2517.186,
            "range": "+/- 5.891",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 3463.575,
            "range": "+/- 10.658",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 2579.34,
            "range": "+/- 8.887",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 19210.588,
            "range": "+/- 130.352",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 3794.879,
            "range": "+/- 35.896",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 7754.156,
            "range": "+/- 25.808",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 3451.428,
            "range": "+/- 13.392",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 4481.162,
            "range": "+/- 13.867",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 3288.242,
            "range": "+/- 12.142",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 20629.503,
            "range": "+/- 54.601",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 29778385.5,
            "range": "+/- 204780.832",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 833.965,
            "range": "+/- 4.823",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 833.253,
            "range": "+/- 3.577",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1082.954,
            "range": "+/- 5.256",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1902.095,
            "range": "+/- 3.513",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 449.933,
            "range": "+/- 1.524",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 652.179,
            "range": "+/- 1.52",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 722.268,
            "range": "+/- 1.074",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 8313.647,
            "range": "+/- 25.579",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 342.196,
            "range": "+/- 2.023",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2246.293,
            "range": "+/- 3.49",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 238.582,
            "range": "+/- 0.698",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 45702.956,
            "range": "+/- 209.604",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 6629.95,
            "range": "+/- 53.484",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 7370.559,
            "range": "+/- 19.409",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 755.999,
            "range": "+/- 2.796",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2248.341,
            "range": "+/- 13.693",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 746.63,
            "range": "+/- 3.552",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 27850.02,
            "range": "+/- 107.896",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 969.219,
            "range": "+/- 5.103",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1278.86,
            "range": "+/- 13.969",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 8551.052,
            "range": "+/- 30.442",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 547.687,
            "range": "+/- 1.297",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 515641.37,
            "range": "+/- 1116.038",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 8106.233,
            "range": "+/- 35.351",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 168.756,
            "range": "+/- 0.39",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2041.423,
            "range": "+/- 2.99",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 74.396,
            "range": "+/- 0.163",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 32594.707,
            "range": "+/- 167.379",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1442.427,
            "range": "+/- 4.447",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1264566.155,
            "range": "+/- 8107.198",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 19833.901,
            "range": "+/- 62.015",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 506.328,
            "range": "+/- 1.516",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 5178.509,
            "range": "+/- 22.882",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 211.615,
            "range": "+/- 0.486",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 77599.989,
            "range": "+/- 264.617",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 4022.077,
            "range": "+/- 26.491",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 4554.292,
            "range": "+/- 63.307",
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
          "id": "9abc7bac9bf6352d7601406803503c28db8b7c69",
          "message": "Merge pull request #169 from nervosys/feat/standby\n\nAdd standby: a sandbox stopped in memory, woken by the next request",
          "timestamp": "2026-10-08T19:05:46-07:00",
          "tree_id": "2f5926af4be30a40e5d51a355476cc514ef911b1",
          "url": "https://github.com/nervosys/HyperMachine/commit/9abc7bac9bf6352d7601406803503c28db8b7c69"
        },
        "date": 1791512627007,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4256.496,
            "range": "+/- 12.398",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 10203.912,
            "range": "+/- 100.048",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3986.486,
            "range": "+/- 14.317",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5333.036,
            "range": "+/- 20.972",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3931.324,
            "range": "+/- 20.584",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 29603.849,
            "range": "+/- 277.315",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5187.275,
            "range": "+/- 10.974",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13735.757,
            "range": "+/- 44.647",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5004.801,
            "range": "+/- 27.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6346.918,
            "range": "+/- 40.729",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4876.391,
            "range": "+/- 20.147",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 28322.359,
            "range": "+/- 179.494",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43391752.5,
            "range": "+/- 146828.982",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 926.828,
            "range": "+/- 5.622",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 960.088,
            "range": "+/- 10.591",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1422.285,
            "range": "+/- 3.476",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2544.306,
            "range": "+/- 9.17",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 595.542,
            "range": "+/- 3.445",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 869.481,
            "range": "+/- 4.077",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 910.14,
            "range": "+/- 1.669",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10629.871,
            "range": "+/- 11.319",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 422.219,
            "range": "+/- 0.787",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2849.185,
            "range": "+/- 2.906",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 302.75,
            "range": "+/- 1.506",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78618.132,
            "range": "+/- 535.198",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12839.565,
            "range": "+/- 234.299",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9671.24,
            "range": "+/- 26.145",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 835.685,
            "range": "+/- 3.233",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2845.91,
            "range": "+/- 12.194",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 832.846,
            "range": "+/- 1.364",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 37530.936,
            "range": "+/- 184.691",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1154.464,
            "range": "+/- 19.369",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1922.657,
            "range": "+/- 7.445",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16451.674,
            "range": "+/- 96.298",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 727.976,
            "range": "+/- 1.037",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 707014.469,
            "range": "+/- 12134.478",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10410.578,
            "range": "+/- 7.407",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 244.606,
            "range": "+/- 0.642",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2685.508,
            "range": "+/- 9.657",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 122.856,
            "range": "+/- 0.486",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41390.318,
            "range": "+/- 25.512",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2198.393,
            "range": "+/- 11.974",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1977013.192,
            "range": "+/- 15744.397",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30420.442,
            "range": "+/- 160.321",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 791.995,
            "range": "+/- 5.42",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7780.896,
            "range": "+/- 29.138",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 349.176,
            "range": "+/- 4.645",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 119993.767,
            "range": "+/- 470.941",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7443.327,
            "range": "+/- 91.399",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8384.353,
            "range": "+/- 97.654",
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
          "id": "dad0b863bf37b6d35c8ae9da4157f05571f961a5",
          "message": "Merge pull request #170 from nervosys/feat/warm-pool\n\nAdd a warm pool: creates take a sandbox restored beforehand",
          "timestamp": "2026-10-08T19:55:15-07:00",
          "tree_id": "6fe54262232a8a4511f2d8939b54e159ba03c1e1",
          "url": "https://github.com/nervosys/HyperMachine/commit/dad0b863bf37b6d35c8ae9da4157f05571f961a5"
        },
        "date": 1791515644035,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4646.108,
            "range": "+/- 42.263",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9626.742,
            "range": "+/- 53.91",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4289.442,
            "range": "+/- 41.879",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 6005.134,
            "range": "+/- 65.02",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4039.545,
            "range": "+/- 20.69",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26983.283,
            "range": "+/- 112.899",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5242.325,
            "range": "+/- 13.973",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13722.969,
            "range": "+/- 43.865",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5036.681,
            "range": "+/- 27.235",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6403.062,
            "range": "+/- 28.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4916.192,
            "range": "+/- 24.494",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 33825.018,
            "range": "+/- 170.182",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 44010164,
            "range": "+/- 263664.685",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 976.067,
            "range": "+/- 14.136",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 948.211,
            "range": "+/- 2.466",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1463.295,
            "range": "+/- 8.567",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2738.455,
            "range": "+/- 58.826",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 615.119,
            "range": "+/- 1.421",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 890.296,
            "range": "+/- 2.456",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 908.863,
            "range": "+/- 1.778",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10612.037,
            "range": "+/- 8.424",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 422.878,
            "range": "+/- 0.777",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2853.567,
            "range": "+/- 3.498",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 301.689,
            "range": "+/- 0.761",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75477.801,
            "range": "+/- 959.054",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 13519.942,
            "range": "+/- 229.302",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9137.886,
            "range": "+/- 47.792",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 832.671,
            "range": "+/- 3.026",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2730.935,
            "range": "+/- 15.781",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 829.867,
            "range": "+/- 1.991",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34250.265,
            "range": "+/- 93.681",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1098.918,
            "range": "+/- 4.861",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2074.282,
            "range": "+/- 36.161",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18208.179,
            "range": "+/- 314.928",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 726.274,
            "range": "+/- 1.886",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 667386.863,
            "range": "+/- 801.232",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10409.79,
            "range": "+/- 8.532",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 240.978,
            "range": "+/- 0.657",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2673.408,
            "range": "+/- 6.234",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 120.835,
            "range": "+/- 0.482",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41469.253,
            "range": "+/- 50.519",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2205.929,
            "range": "+/- 8.893",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1932704.36,
            "range": "+/- 6872.614",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30895.638,
            "range": "+/- 327.278",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 791.034,
            "range": "+/- 3.004",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7879.344,
            "range": "+/- 33.655",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 346.871,
            "range": "+/- 2.932",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 120995.08,
            "range": "+/- 472.49",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7304.907,
            "range": "+/- 42.753",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8088.526,
            "range": "+/- 52.655",
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
          "id": "6eaf06f0f3320a81274858273a01afb3b4404a03",
          "message": "Merge pull request #171 from nervosys/feat/disk-pause\n\nLet a sandbox that holds a disk pause and resume",
          "timestamp": "2026-10-08T20:19:42-07:00",
          "tree_id": "026b2efac3ae9361c64d46c117dbf1ef563af294",
          "url": "https://github.com/nervosys/HyperMachine/commit/6eaf06f0f3320a81274858273a01afb3b4404a03"
        },
        "date": 1791517274084,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4396.013,
            "range": "+/- 35.076",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9831.177,
            "range": "+/- 65.81",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4150.503,
            "range": "+/- 22.591",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5288.713,
            "range": "+/- 14.932",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 4017.554,
            "range": "+/- 26.595",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 28007.488,
            "range": "+/- 375.093",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5224.834,
            "range": "+/- 23.003",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13875.487,
            "range": "+/- 56.562",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 5378.349,
            "range": "+/- 52.057",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6311.942,
            "range": "+/- 11.224",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4861.675,
            "range": "+/- 14.22",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 30235.204,
            "range": "+/- 339.459",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45159006.5,
            "range": "+/- 314938.46",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 1224.302,
            "range": "+/- 27.636",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 1102.56,
            "range": "+/- 16.633",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1700.921,
            "range": "+/- 12.133",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 3013.378,
            "range": "+/- 23.266",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 658.115,
            "range": "+/- 5.345",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 1020.677,
            "range": "+/- 11.45",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 916.715,
            "range": "+/- 3.195",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10705.495,
            "range": "+/- 18.699",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 428.725,
            "range": "+/- 2.273",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2863.842,
            "range": "+/- 6.496",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 305.574,
            "range": "+/- 1.765",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72470.536,
            "range": "+/- 711.756",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12287.414,
            "range": "+/- 41.179",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9439.573,
            "range": "+/- 23.219",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 969.394,
            "range": "+/- 6.331",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2773.058,
            "range": "+/- 6.241",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 831.182,
            "range": "+/- 2.362",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 36166.675,
            "range": "+/- 70.423",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1120.062,
            "range": "+/- 5.27",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2114.181,
            "range": "+/- 17.309",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16256.398,
            "range": "+/- 66.293",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 732.305,
            "range": "+/- 1.511",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663399.998,
            "range": "+/- 549.207",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10426.374,
            "range": "+/- 6.455",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.533,
            "range": "+/- 0.637",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2665.377,
            "range": "+/- 1.796",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 121.234,
            "range": "+/- 0.477",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41493.288,
            "range": "+/- 22.806",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2303.049,
            "range": "+/- 25.163",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2066629.615,
            "range": "+/- 25069.893",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31634.993,
            "range": "+/- 338.173",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 828.691,
            "range": "+/- 9.366",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8500.192,
            "range": "+/- 150.845",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 354.316,
            "range": "+/- 4.865",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125789.541,
            "range": "+/- 1048.521",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7313.964,
            "range": "+/- 76.86",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8159.212,
            "range": "+/- 35.202",
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
          "id": "0c36d10dca54da457137c49822ee598170ef2f91",
          "message": "Merge pull request #172 from nervosys/feat/disk-fork\n\nFork a sandbox together with the disk it holds",
          "timestamp": "2026-10-08T21:07:41-07:00",
          "tree_id": "4fa0f6a2b5007d2b44646f36ac3ad086720c03c3",
          "url": "https://github.com/nervosys/HyperMachine/commit/0c36d10dca54da457137c49822ee598170ef2f91"
        },
        "date": 1791519886870,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4261.564,
            "range": "+/- 11.57",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9650.001,
            "range": "+/- 45.684",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 4012.803,
            "range": "+/- 9.77",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5325.274,
            "range": "+/- 27.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3915.145,
            "range": "+/- 9.293",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 27048.373,
            "range": "+/- 117.132",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5210.182,
            "range": "+/- 11.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 11195.785,
            "range": "+/- 52.705",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4960.833,
            "range": "+/- 21.938",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6327.492,
            "range": "+/- 21.121",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4869.511,
            "range": "+/- 20.252",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29411.671,
            "range": "+/- 91.484",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43188392,
            "range": "+/- 113453.896",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 927,
            "range": "+/- 5.24",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 925.752,
            "range": "+/- 3.258",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1434.528,
            "range": "+/- 4.389",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2625.643,
            "range": "+/- 17.517",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 589.101,
            "range": "+/- 1.123",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 862.397,
            "range": "+/- 1.145",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 907.746,
            "range": "+/- 0.974",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10639.07,
            "range": "+/- 10.246",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 422.069,
            "range": "+/- 0.95",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2866.167,
            "range": "+/- 5.372",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 300.333,
            "range": "+/- 0.709",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74764.6,
            "range": "+/- 364.358",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12454.39,
            "range": "+/- 46.857",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9084.605,
            "range": "+/- 32.382",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 864.943,
            "range": "+/- 6.645",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2688.299,
            "range": "+/- 5.714",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 872.412,
            "range": "+/- 6.08",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 36946.619,
            "range": "+/- 691.076",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1186.269,
            "range": "+/- 10.099",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1953.37,
            "range": "+/- 9.79",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16657.099,
            "range": "+/- 114.268",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 725.385,
            "range": "+/- 1.041",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 695024.862,
            "range": "+/- 14170.268",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10411.979,
            "range": "+/- 8.327",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 240.317,
            "range": "+/- 0.43",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2663.227,
            "range": "+/- 2.241",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.22,
            "range": "+/- 0.206",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41421.964,
            "range": "+/- 30.346",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2315.758,
            "range": "+/- 28.264",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1981422.985,
            "range": "+/- 19037.052",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30160.931,
            "range": "+/- 95.232",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 968.334,
            "range": "+/- 25.13",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7810.243,
            "range": "+/- 29.225",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 350.473,
            "range": "+/- 2.573",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 119830.586,
            "range": "+/- 555.743",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7164.047,
            "range": "+/- 39.376",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8207.014,
            "range": "+/- 52.886",
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
          "id": "d624f7353816feea75c0fa8d7582e35c81b1dba5",
          "message": "Merge pull request #174 from nervosys/feat/disk-template-restore\n\nAdd a disk slot: restore a sandbox with a disk from a template",
          "timestamp": "2026-10-09T07:14:17-07:00",
          "tree_id": "3284183b81d1c664b84737f9624b00f0dbc97574",
          "url": "https://github.com/nervosys/HyperMachine/commit/d624f7353816feea75c0fa8d7582e35c81b1dba5"
        },
        "date": 1791556352218,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4251.975,
            "range": "+/- 16.11",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9586.301,
            "range": "+/- 29.047",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3976.909,
            "range": "+/- 4.515",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5260.519,
            "range": "+/- 11.13",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3908.067,
            "range": "+/- 10.127",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26944.076,
            "range": "+/- 155.987",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5221.987,
            "range": "+/- 14.842",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13755.58,
            "range": "+/- 32.793",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4947.147,
            "range": "+/- 14.06",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6355.222,
            "range": "+/- 21.679",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4873.801,
            "range": "+/- 14.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29445.298,
            "range": "+/- 152.815",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43434790,
            "range": "+/- 146938.695",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 920.785,
            "range": "+/- 3.827",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 923.235,
            "range": "+/- 3.222",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1465.819,
            "range": "+/- 8.173",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2637.387,
            "range": "+/- 18.367",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 603.597,
            "range": "+/- 1.342",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 908.074,
            "range": "+/- 6.434",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 913.533,
            "range": "+/- 1.79",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10605.004,
            "range": "+/- 17.069",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 425.29,
            "range": "+/- 0.941",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2851.962,
            "range": "+/- 7.545",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 301.075,
            "range": "+/- 0.783",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73052.731,
            "range": "+/- 504.774",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12556.333,
            "range": "+/- 112.69",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9481.218,
            "range": "+/- 42.332",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 874.555,
            "range": "+/- 6.253",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2777.785,
            "range": "+/- 8.583",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 874.936,
            "range": "+/- 5.646",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 36378.557,
            "range": "+/- 173.4",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1164.304,
            "range": "+/- 9.647",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2215.277,
            "range": "+/- 18.893",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16554.465,
            "range": "+/- 103.303",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 724.013,
            "range": "+/- 0.518",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664673.533,
            "range": "+/- 566.824",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10412.724,
            "range": "+/- 9.342",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 240.749,
            "range": "+/- 0.407",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2659.107,
            "range": "+/- 1.798",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.186,
            "range": "+/- 0.193",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41532.29,
            "range": "+/- 24.094",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2193.06,
            "range": "+/- 8.696",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1939218.66,
            "range": "+/- 12125.698",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30379.689,
            "range": "+/- 169.26",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 801.331,
            "range": "+/- 6.178",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7838.768,
            "range": "+/- 37.988",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 338.231,
            "range": "+/- 1.886",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 120207.601,
            "range": "+/- 410.823",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7872.001,
            "range": "+/- 84.962",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8095.809,
            "range": "+/- 39.055",
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
          "id": "3ea78daedbcd5557a8c5eaf21894f2a94ab50932",
          "message": "Merge pull request #176 from nervosys/feat/windows-appcontainer\n\nEnforce \"no network\" for the Windows process sandbox",
          "timestamp": "2026-10-09T10:00:51-07:00",
          "tree_id": "4b7cead7ed2e2e8a398701149510a705236c2ee5",
          "url": "https://github.com/nervosys/HyperMachine/commit/3ea78daedbcd5557a8c5eaf21894f2a94ab50932"
        },
        "date": 1791566298638,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4262.051,
            "range": "+/- 17.367",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9663.013,
            "range": "+/- 62.589",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3988.281,
            "range": "+/- 10.433",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5256.214,
            "range": "+/- 5.959",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3922.676,
            "range": "+/- 13.562",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26990.941,
            "range": "+/- 119.869",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5185.681,
            "range": "+/- 11.624",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 13652.856,
            "range": "+/- 61.19",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4922.22,
            "range": "+/- 11.735",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6314.009,
            "range": "+/- 21.417",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4844.677,
            "range": "+/- 18.921",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29591.838,
            "range": "+/- 110.961",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 45963154.5,
            "range": "+/- 810808.628",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 898.702,
            "range": "+/- 1.58",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 920.661,
            "range": "+/- 5.088",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1426.956,
            "range": "+/- 5.019",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2535.186,
            "range": "+/- 10.397",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 598.726,
            "range": "+/- 2.333",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 876.182,
            "range": "+/- 2.508",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 905.455,
            "range": "+/- 2.123",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10618.818,
            "range": "+/- 10.516",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 421.394,
            "range": "+/- 0.649",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2854.068,
            "range": "+/- 7.62",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 306.223,
            "range": "+/- 2.876",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 71778.012,
            "range": "+/- 238.697",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12417.984,
            "range": "+/- 70.887",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9058.969,
            "range": "+/- 46.642",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 831.431,
            "range": "+/- 4.596",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2833.817,
            "range": "+/- 20.742",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 856.144,
            "range": "+/- 7.568",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34601.664,
            "range": "+/- 179.081",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1120.694,
            "range": "+/- 8.77",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2003.568,
            "range": "+/- 21.937",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17029.905,
            "range": "+/- 126.38",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 730.286,
            "range": "+/- 2.486",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663221.832,
            "range": "+/- 595.009",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10398.34,
            "range": "+/- 5.962",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 239.649,
            "range": "+/- 0.171",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2659.166,
            "range": "+/- 1.682",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.304,
            "range": "+/- 0.207",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41398.383,
            "range": "+/- 27.887",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2199.654,
            "range": "+/- 9.255",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1971081.44,
            "range": "+/- 14271.693",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31277.497,
            "range": "+/- 252.8",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 793.019,
            "range": "+/- 3.494",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7823.937,
            "range": "+/- 30.545",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 341.679,
            "range": "+/- 1.308",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143445.842,
            "range": "+/- 3093.913",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7374.427,
            "range": "+/- 27.588",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8202.671,
            "range": "+/- 51.59",
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
          "id": "3da8bc9ea9a9e3db048a592f3e1ea8f466dd74cf",
          "message": "Merge pull request #177 from nervosys/feat/windows-path-grants\n\nGrant a sandboxed program the paths it needs",
          "timestamp": "2026-10-09T10:34:03-07:00",
          "tree_id": "f358afac739b243f37bc3bebbe50b3ecca0f565c",
          "url": "https://github.com/nervosys/HyperMachine/commit/3da8bc9ea9a9e3db048a592f3e1ea8f466dd74cf"
        },
        "date": 1791568348466,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 4254.372,
            "range": "+/- 12.319",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9535.41,
            "range": "+/- 21.488",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 3990.426,
            "range": "+/- 9.62",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 5303.122,
            "range": "+/- 21.564",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 3916.302,
            "range": "+/- 14.485",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 26973.815,
            "range": "+/- 131.222",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 5260.661,
            "range": "+/- 19.623",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 12489.967,
            "range": "+/- 77.949",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 4943.356,
            "range": "+/- 12.423",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 6357.314,
            "range": "+/- 20.189",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 4909.288,
            "range": "+/- 26.405",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 29482.037,
            "range": "+/- 144.485",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 43399497,
            "range": "+/- 72259.602",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 939.53,
            "range": "+/- 4.06",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 951.152,
            "range": "+/- 6.791",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1519.372,
            "range": "+/- 12.976",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2744.789,
            "range": "+/- 26.252",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 603.443,
            "range": "+/- 2.752",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 1057.906,
            "range": "+/- 21.298",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 923.416,
            "range": "+/- 3.509",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10633.248,
            "range": "+/- 10.176",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 437.58,
            "range": "+/- 2.477",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2861.468,
            "range": "+/- 6.977",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 305.026,
            "range": "+/- 1.295",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75662.895,
            "range": "+/- 955.391",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12920.964,
            "range": "+/- 139.007",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 9657.291,
            "range": "+/- 212.952",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 883.747,
            "range": "+/- 5.369",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 2678.591,
            "range": "+/- 13.429",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 841.117,
            "range": "+/- 5.586",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 34721.72,
            "range": "+/- 334.514",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 1090.467,
            "range": "+/- 5.179",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2087.765,
            "range": "+/- 35.959",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17295.913,
            "range": "+/- 296.48",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 724.35,
            "range": "+/- 0.553",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663373.788,
            "range": "+/- 639.203",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10440.015,
            "range": "+/- 7.252",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 241.355,
            "range": "+/- 0.694",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2677.42,
            "range": "+/- 4.567",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 119.182,
            "range": "+/- 0.149",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41537.682,
            "range": "+/- 96.396",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2281.765,
            "range": "+/- 33.131",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1981155.858,
            "range": "+/- 12146.401",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 30490.509,
            "range": "+/- 163.296",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 807.376,
            "range": "+/- 5.915",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7907.524,
            "range": "+/- 56.951",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 340.93,
            "range": "+/- 1.83",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 120720.172,
            "range": "+/- 358.106",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7222.201,
            "range": "+/- 38.024",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8114.326,
            "range": "+/- 42.144",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}