window.BENCHMARK_DATA = {
  "lastUpdate": 1788364868695,
  "repoUrl": "https://github.com/nervosys/HyperMachine",
  "entries": {
    "HyperMachine Benchmarks": [
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "e4ed78673bc9edd0f7927779c554fa87180b910c",
          "message": "fix(ci): the Benchmarks job has never run to completion",
          "timestamp": "2026-09-01T22:01:44Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/74/commits/e4ed78673bc9edd0f7927779c554fa87180b910c"
        },
        "date": 1788301948672,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 513.95,
            "range": "+/- 15.822",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2706.851,
            "range": "+/- 12.174",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 357.307,
            "range": "+/- 1.066",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 834.581,
            "range": "+/- 6.18",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 332.108,
            "range": "+/- 0.52",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9846.936,
            "range": "+/- 17.971",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 514.048,
            "range": "+/- 1.338",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2962.782,
            "range": "+/- 7.463",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 403.359,
            "range": "+/- 0.378",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 945.562,
            "range": "+/- 4.041",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 376.301,
            "range": "+/- 0.588",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 15908.524,
            "range": "+/- 54.162",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2102.122,
            "range": "+/- 2.447",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 83.971,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 92.327,
            "range": "+/- 0.311",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 995.135,
            "range": "+/- 4.234",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1602.038,
            "range": "+/- 11.688",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 560.345,
            "range": "+/- 1.385",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 703.506,
            "range": "+/- 1.817",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 821.846,
            "range": "+/- 1.12",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9329.063,
            "range": "+/- 11.478",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 387.446,
            "range": "+/- 0.34",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2514.211,
            "range": "+/- 1.37",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 280.075,
            "range": "+/- 0.472",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 58860.446,
            "range": "+/- 198.278",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8930.957,
            "range": "+/- 68.706",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 304.474,
            "range": "+/- 1.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.294,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 76.813,
            "range": "+/- 0.098",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 16.877,
            "range": "+/- 0.016",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1212.688,
            "range": "+/- 2.965",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 27.051,
            "range": "+/- 0.076",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1565.768,
            "range": "+/- 5.828",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11813.891,
            "range": "+/- 35.296",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 650.156,
            "range": "+/- 0.484",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579924.496,
            "range": "+/- 941.664",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9125.918,
            "range": "+/- 4.934",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 215.244,
            "range": "+/- 0.312",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2345.035,
            "range": "+/- 0.876",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 108.581,
            "range": "+/- 0.279",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36247.847,
            "range": "+/- 16.885",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2004.663,
            "range": "+/- 23.083",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1733955.437,
            "range": "+/- 4967.609",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27314.991,
            "range": "+/- 53.548",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 687.022,
            "range": "+/- 1.226",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7182.414,
            "range": "+/- 55.111",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.017,
            "range": "+/- 0.851",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 109233.863,
            "range": "+/- 350.639",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5221.351,
            "range": "+/- 19.21",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5893.669,
            "range": "+/- 28.357",
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
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "dda660158b0660bad2f394af2729e31d0a2e8000",
          "message": "deps: update hashicorp/aws requirement from ~> 5.0 to ~> 6.62 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/72/commits/dda660158b0660bad2f394af2729e31d0a2e8000"
        },
        "date": 1788305683843,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 506.854,
            "range": "+/- 3.055",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3300.177,
            "range": "+/- 11.294",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 374.765,
            "range": "+/- 1.471",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 986.169,
            "range": "+/- 3.886",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 339.129,
            "range": "+/- 1.116",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12753.792,
            "range": "+/- 71.36",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 581.756,
            "range": "+/- 4.874",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6515.982,
            "range": "+/- 34.255",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 531.889,
            "range": "+/- 9.974",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1114.335,
            "range": "+/- 7.322",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 409.102,
            "range": "+/- 2.519",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13333.335,
            "range": "+/- 88.692",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2481.887,
            "range": "+/- 14.338",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.152,
            "range": "+/- 0.437",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 113.594,
            "range": "+/- 0.827",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1093.459,
            "range": "+/- 3.687",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1757.398,
            "range": "+/- 2.114",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 586.455,
            "range": "+/- 2.498",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 751.406,
            "range": "+/- 2.561",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 946.278,
            "range": "+/- 2.247",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10761.921,
            "range": "+/- 107.29",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 455.225,
            "range": "+/- 1.907",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2908.796,
            "range": "+/- 4.648",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 337.096,
            "range": "+/- 2.284",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72763.175,
            "range": "+/- 490.571",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12775.923,
            "range": "+/- 174.138",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 355.316,
            "range": "+/- 1.656",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.776,
            "range": "+/- 0.086",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 97.731,
            "range": "+/- 0.546",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.972,
            "range": "+/- 0.071",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1387.499,
            "range": "+/- 1.791",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.041,
            "range": "+/- 0.15",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2060.359,
            "range": "+/- 26.998",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 19594.605,
            "range": "+/- 548.605",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.324,
            "range": "+/- 0.736",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662771.679,
            "range": "+/- 381.827",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10446.827,
            "range": "+/- 21.359",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.37,
            "range": "+/- 0.663",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2694.683,
            "range": "+/- 2.135",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 127.244,
            "range": "+/- 0.135",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41469.119,
            "range": "+/- 33.74",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2264.794,
            "range": "+/- 11.227",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2099206.88,
            "range": "+/- 21624.098",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31661.829,
            "range": "+/- 152.627",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 811.799,
            "range": "+/- 4.166",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8091.481,
            "range": "+/- 29.302",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 300.272,
            "range": "+/- 1.889",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124603.241,
            "range": "+/- 430.643",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7238.834,
            "range": "+/- 21.745",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8238.362,
            "range": "+/- 69.303",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "adc2ee5e8128aa96ca7c86e0b35f1d73a98b5a9d",
          "message": "deps(deps): bump tock-registers from 0.9.0 to 0.10.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/48/commits/adc2ee5e8128aa96ca7c86e0b35f1d73a98b5a9d"
        },
        "date": 1788305852192,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 509.202,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3435.647,
            "range": "+/- 15.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 398.6,
            "range": "+/- 1.107",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1011.234,
            "range": "+/- 2.605",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 359.174,
            "range": "+/- 1.674",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12385.347,
            "range": "+/- 39.312",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 581.446,
            "range": "+/- 1.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6407.883,
            "range": "+/- 16.424",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 450.66,
            "range": "+/- 3.709",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1116.455,
            "range": "+/- 3.691",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 413.402,
            "range": "+/- 2.14",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12653.348,
            "range": "+/- 50.986",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2446.015,
            "range": "+/- 8.562",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.708,
            "range": "+/- 0.4",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 113.701,
            "range": "+/- 0.788",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1066.15,
            "range": "+/- 1.783",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1718.744,
            "range": "+/- 5.951",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 590.019,
            "range": "+/- 2.832",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 747.063,
            "range": "+/- 2.189",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 934.014,
            "range": "+/- 1.417",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10630.007,
            "range": "+/- 8.809",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 442.65,
            "range": "+/- 0.496",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2881.477,
            "range": "+/- 5.776",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 322.53,
            "range": "+/- 1.16",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72738.695,
            "range": "+/- 391.148",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12971.38,
            "range": "+/- 266.891",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 352.23,
            "range": "+/- 0.808",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.652,
            "range": "+/- 0.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.676,
            "range": "+/- 0.293",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.561,
            "range": "+/- 0.047",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1396.667,
            "range": "+/- 3.471",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.772,
            "range": "+/- 0.101",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1941.686,
            "range": "+/- 8.511",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17489.81,
            "range": "+/- 344.25",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 730.412,
            "range": "+/- 0.97",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662907.903,
            "range": "+/- 885.859",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10434.363,
            "range": "+/- 10.23",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 246.605,
            "range": "+/- 0.426",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2682.061,
            "range": "+/- 2.442",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 125.007,
            "range": "+/- 0.216",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41662.016,
            "range": "+/- 149.987",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2238.889,
            "range": "+/- 5.072",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1995319.623,
            "range": "+/- 6784.926",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31474.844,
            "range": "+/- 215.025",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 797.423,
            "range": "+/- 3.558",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8076.132,
            "range": "+/- 34.276",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 299.961,
            "range": "+/- 2.683",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125277.841,
            "range": "+/- 844.192",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7252.539,
            "range": "+/- 34.525",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8225.726,
            "range": "+/- 80.958",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "67faa0ec030673370880d7d57f7d6e54f646016d",
          "message": "deps: bump rust from 1.87-bookworm to 1.98-bookworm",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/62/commits/67faa0ec030673370880d7d57f7d6e54f646016d"
        },
        "date": 1788305952028,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 554.755,
            "range": "+/- 2.587",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3415.883,
            "range": "+/- 8.147",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 428.37,
            "range": "+/- 1.057",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1032.034,
            "range": "+/- 3.011",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 396.441,
            "range": "+/- 2.443",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12496.44,
            "range": "+/- 71.651",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 645.606,
            "range": "+/- 1.636",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3899.582,
            "range": "+/- 27.703",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 496.036,
            "range": "+/- 1.276",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1227.066,
            "range": "+/- 1.506",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 458.322,
            "range": "+/- 2.207",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13407.391,
            "range": "+/- 52.583",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2793.359,
            "range": "+/- 17.243",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 114.359,
            "range": "+/- 1.449",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 133.434,
            "range": "+/- 1.908",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1186.583,
            "range": "+/- 2.864",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1948.275,
            "range": "+/- 12.889",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 645.123,
            "range": "+/- 1.414",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 823.03,
            "range": "+/- 2.086",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1068.285,
            "range": "+/- 2.072",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12012.241,
            "range": "+/- 10.79",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 502.326,
            "range": "+/- 1.136",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3257.868,
            "range": "+/- 6.209",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.075,
            "range": "+/- 1.145",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75690.033,
            "range": "+/- 477.675",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11318.613,
            "range": "+/- 94.956",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 421.062,
            "range": "+/- 2.772",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.908,
            "range": "+/- 0.106",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.762,
            "range": "+/- 0.392",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.228,
            "range": "+/- 0.095",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1661.264,
            "range": "+/- 10.936",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.395,
            "range": "+/- 0.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2078.305,
            "range": "+/- 11.644",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15822.829,
            "range": "+/- 240.86",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.036,
            "range": "+/- 0.685",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747725.1,
            "range": "+/- 284.704",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11782.974,
            "range": "+/- 5.365",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.37,
            "range": "+/- 0.495",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3027.138,
            "range": "+/- 2.424",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.368,
            "range": "+/- 0.516",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46782.6,
            "range": "+/- 19.939",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2620.919,
            "range": "+/- 47.936",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2235981.348,
            "range": "+/- 6009.782",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 41765.008,
            "range": "+/- 723.334",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 891.768,
            "range": "+/- 2.383",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9630.846,
            "range": "+/- 129.376",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 326.917,
            "range": "+/- 1.145",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143845.168,
            "range": "+/- 1393.146",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6896.055,
            "range": "+/- 67.198",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7767.208,
            "range": "+/- 45.761",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "cc7f78b8685986a9685c8aee82c0877930208b4d",
          "message": "ci: bump actions/download-artifact from 4 to 8",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/37/commits/cc7f78b8685986a9685c8aee82c0877930208b4d"
        },
        "date": 1788308064012,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 516.927,
            "range": "+/- 2.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3384.491,
            "range": "+/- 30.941",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 396.576,
            "range": "+/- 0.997",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.925,
            "range": "+/- 4.936",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 370.99,
            "range": "+/- 2.853",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12586.476,
            "range": "+/- 35.884",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 574.197,
            "range": "+/- 2.724",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6018.127,
            "range": "+/- 60.578",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 459.945,
            "range": "+/- 4.441",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1106.403,
            "range": "+/- 4.694",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 425.699,
            "range": "+/- 2.941",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 47099.582,
            "range": "+/- 150.474",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2471.019,
            "range": "+/- 12.974",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.78,
            "range": "+/- 0.466",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.917,
            "range": "+/- 0.723",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1071.592,
            "range": "+/- 5.162",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1708.828,
            "range": "+/- 2.586",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 629.641,
            "range": "+/- 5.453",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 799.826,
            "range": "+/- 17.495",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 945.497,
            "range": "+/- 2.881",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10677.277,
            "range": "+/- 9.215",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 444.759,
            "range": "+/- 0.819",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2906.069,
            "range": "+/- 4.858",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 322.048,
            "range": "+/- 1.581",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77955.345,
            "range": "+/- 1449.124",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12573.846,
            "range": "+/- 151.036",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.051,
            "range": "+/- 0.938",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.534,
            "range": "+/- 0.091",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.53,
            "range": "+/- 0.497",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.739,
            "range": "+/- 0.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1406.013,
            "range": "+/- 2.815",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.158,
            "range": "+/- 0.218",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2100.628,
            "range": "+/- 28.345",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17247.339,
            "range": "+/- 245.383",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 737.267,
            "range": "+/- 1.269",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664195.6,
            "range": "+/- 1604.653",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10426.715,
            "range": "+/- 5.288",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.581,
            "range": "+/- 0.466",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2681.527,
            "range": "+/- 3.091",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.975,
            "range": "+/- 0.534",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41586.607,
            "range": "+/- 33.41",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2276.237,
            "range": "+/- 13.332",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1986841.615,
            "range": "+/- 6347.758",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31591.145,
            "range": "+/- 152.731",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 796.25,
            "range": "+/- 3.111",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8141.219,
            "range": "+/- 27.981",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.405,
            "range": "+/- 1.582",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124504.272,
            "range": "+/- 346.724",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7566.595,
            "range": "+/- 84.231",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8368.764,
            "range": "+/- 69.379",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "3b9e6639a3079e70e8da160d6b26ef8bf8312c00",
          "message": "ci: bump hashicorp/setup-terraform from 3 to 4",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/39/commits/3b9e6639a3079e70e8da160d6b26ef8bf8312c00"
        },
        "date": 1788308147992,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 428.298,
            "range": "+/- 1.239",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2711.749,
            "range": "+/- 6.89",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 330.033,
            "range": "+/- 1.18",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 848.859,
            "range": "+/- 3.07",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 307.919,
            "range": "+/- 0.936",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11627.45,
            "range": "+/- 32.665",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 507.205,
            "range": "+/- 2.153",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3387.69,
            "range": "+/- 17.543",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 388.854,
            "range": "+/- 1.177",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 981.616,
            "range": "+/- 4.265",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 367.575,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10715.664,
            "range": "+/- 60.28",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2528.186,
            "range": "+/- 69.025",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 92.352,
            "range": "+/- 0.564",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 104.681,
            "range": "+/- 0.311",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1116.245,
            "range": "+/- 2.897",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1801.612,
            "range": "+/- 2.797",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 596.707,
            "range": "+/- 1.48",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 766.427,
            "range": "+/- 1.546",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 963.896,
            "range": "+/- 2.911",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11281.251,
            "range": "+/- 12.871",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 438.727,
            "range": "+/- 1.23",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3019.381,
            "range": "+/- 4.183",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 309.072,
            "range": "+/- 0.762",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73023.512,
            "range": "+/- 428.876",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12057.605,
            "range": "+/- 86.3",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 412.709,
            "range": "+/- 3.366",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 15.713,
            "range": "+/- 0.104",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.802,
            "range": "+/- 0.615",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.252,
            "range": "+/- 0.154",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1643.777,
            "range": "+/- 12.845",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.791,
            "range": "+/- 0.488",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2190.028,
            "range": "+/- 44.31",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16867.17,
            "range": "+/- 115.892",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 815.176,
            "range": "+/- 16.584",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 714130.122,
            "range": "+/- 885.722",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 12247.377,
            "range": "+/- 287.622",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 248.872,
            "range": "+/- 0.448",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2899.82,
            "range": "+/- 6.766",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 118.453,
            "range": "+/- 0.516",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 44824.428,
            "range": "+/- 160.725",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2065.322,
            "range": "+/- 17.787",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1802555.376,
            "range": "+/- 6924.071",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 28127.719,
            "range": "+/- 55.484",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 734.414,
            "range": "+/- 5.033",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7311.222,
            "range": "+/- 32.852",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 275.082,
            "range": "+/- 1.026",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 115286.074,
            "range": "+/- 934.609",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7178.771,
            "range": "+/- 46.007",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7919.883,
            "range": "+/- 24.679",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "38f486aac80732efd4837fcc418867e3879b1e74",
          "message": "ci: bump azure/setup-kubectl from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/36/commits/38f486aac80732efd4837fcc418867e3879b1e74"
        },
        "date": 1788308182841,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 582.972,
            "range": "+/- 3.597",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3744.409,
            "range": "+/- 38.905",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 476.766,
            "range": "+/- 4.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1058.362,
            "range": "+/- 4.379",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 410.244,
            "range": "+/- 2.126",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12831.981,
            "range": "+/- 78.015",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 635.572,
            "range": "+/- 2.96",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4107.545,
            "range": "+/- 47.452",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 504.211,
            "range": "+/- 3.451",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1231.859,
            "range": "+/- 10.38",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 464.928,
            "range": "+/- 1.977",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 14027.206,
            "range": "+/- 126.177",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2756.103,
            "range": "+/- 14.373",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 124.017,
            "range": "+/- 2.73",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 138.665,
            "range": "+/- 1.298",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1211.605,
            "range": "+/- 5.178",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1994.266,
            "range": "+/- 10.416",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 673.635,
            "range": "+/- 4.984",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 837.76,
            "range": "+/- 2.573",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1058.155,
            "range": "+/- 2.012",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12045.398,
            "range": "+/- 14.489",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.401,
            "range": "+/- 1.815",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3260.256,
            "range": "+/- 5.198",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.758,
            "range": "+/- 1.516",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 80177.221,
            "range": "+/- 1208.743",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12814.291,
            "range": "+/- 276.754",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 470.847,
            "range": "+/- 3.494",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.63,
            "range": "+/- 0.209",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 115.636,
            "range": "+/- 0.912",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.704,
            "range": "+/- 0.237",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1852.729,
            "range": "+/- 11.964",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 37.007,
            "range": "+/- 0.3",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2324.13,
            "range": "+/- 29.061",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18691.882,
            "range": "+/- 756.708",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 843.187,
            "range": "+/- 0.915",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 751516.302,
            "range": "+/- 1522.368",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11922.915,
            "range": "+/- 29.523",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 279.425,
            "range": "+/- 0.393",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3060.188,
            "range": "+/- 5.8",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 144.626,
            "range": "+/- 0.799",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46852.136,
            "range": "+/- 34.565",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2524.487,
            "range": "+/- 6.392",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2319981.217,
            "range": "+/- 23778.012",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 37692.187,
            "range": "+/- 464.52",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 938.565,
            "range": "+/- 11.093",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9208.797,
            "range": "+/- 48.04",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 353.481,
            "range": "+/- 5.452",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 141104.756,
            "range": "+/- 938.706",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6937.378,
            "range": "+/- 70.157",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8164.177,
            "range": "+/- 135.143",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "c3cf460cb765ef4e578943da2840dff482a09e0b",
          "message": "ci: bump aws-actions/configure-aws-credentials from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/40/commits/c3cf460cb765ef4e578943da2840dff482a09e0b"
        },
        "date": 1788308205071,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 541.154,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3372.298,
            "range": "+/- 14.271",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 398.669,
            "range": "+/- 0.555",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1041.477,
            "range": "+/- 9.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 373.372,
            "range": "+/- 1.472",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12642.196,
            "range": "+/- 71.108",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 658.014,
            "range": "+/- 3.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3845.953,
            "range": "+/- 25.548",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 487.174,
            "range": "+/- 2.273",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1221.994,
            "range": "+/- 11.862",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.123,
            "range": "+/- 1.481",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12272.874,
            "range": "+/- 50.082",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2697.673,
            "range": "+/- 16.882",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 106.248,
            "range": "+/- 2.068",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.79,
            "range": "+/- 0.296",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1208.765,
            "range": "+/- 1.328",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1942.064,
            "range": "+/- 6.92",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 675.965,
            "range": "+/- 4.846",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 872.862,
            "range": "+/- 5.004",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1057.532,
            "range": "+/- 1.164",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 13024.787,
            "range": "+/- 106.333",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 496.864,
            "range": "+/- 0.58",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3248.981,
            "range": "+/- 2.089",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 359.406,
            "range": "+/- 0.88",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 81425.818,
            "range": "+/- 922.168",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12817.206,
            "range": "+/- 301.813",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 396.114,
            "range": "+/- 1.527",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.706,
            "range": "+/- 0.219",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 99.536,
            "range": "+/- 0.261",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.877,
            "range": "+/- 0.105",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1616.193,
            "range": "+/- 6.741",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.337,
            "range": "+/- 0.116",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2488.96,
            "range": "+/- 27.269",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 20714.597,
            "range": "+/- 386.58",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 838.456,
            "range": "+/- 0.649",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 752415.818,
            "range": "+/- 1090.888",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11794.773,
            "range": "+/- 8.927",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.296,
            "range": "+/- 0.55",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3029.623,
            "range": "+/- 1.304",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.529,
            "range": "+/- 0.662",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46909.462,
            "range": "+/- 31.416",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2546.87,
            "range": "+/- 10.495",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2243169.273,
            "range": "+/- 8761.3",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 37207.842,
            "range": "+/- 574.269",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 906.069,
            "range": "+/- 6.759",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9116.315,
            "range": "+/- 37.471",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 334.13,
            "range": "+/- 2.037",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143983.148,
            "range": "+/- 1109.593",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7537.39,
            "range": "+/- 101.02",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8406.807,
            "range": "+/- 145.402",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "561436fd4f67d4b8f80a06c072e7c79e341541c6",
          "message": "ci: bump azure/k8s-set-context from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/38/commits/561436fd4f67d4b8f80a06c072e7c79e341541c6"
        },
        "date": 1788308714857,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 423.504,
            "range": "+/- 2.78",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2607.524,
            "range": "+/- 13.89",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 556.878,
            "range": "+/- 5.307",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 821.488,
            "range": "+/- 6.704",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 305.117,
            "range": "+/- 0.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11144.962,
            "range": "+/- 34.624",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 455.962,
            "range": "+/- 2.643",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3140.001,
            "range": "+/- 9.586",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 360.043,
            "range": "+/- 1.243",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 914.237,
            "range": "+/- 8.085",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 342.174,
            "range": "+/- 1.609",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10078.398,
            "range": "+/- 40.066",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2005.99,
            "range": "+/- 5.855",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 85.826,
            "range": "+/- 0.738",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 94.966,
            "range": "+/- 0.399",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1043.014,
            "range": "+/- 7.525",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1634.73,
            "range": "+/- 9.345",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 549.37,
            "range": "+/- 2.757",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 697.633,
            "range": "+/- 3.746",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 889.495,
            "range": "+/- 5.351",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10232.973,
            "range": "+/- 46.885",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 402.007,
            "range": "+/- 1.216",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2759.908,
            "range": "+/- 8.214",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 284.498,
            "range": "+/- 0.701",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59401.145,
            "range": "+/- 447.552",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 10946.808,
            "range": "+/- 82.999",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 354.168,
            "range": "+/- 1.055",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 14.531,
            "range": "+/- 0.151",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 92.984,
            "range": "+/- 0.315",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 19.147,
            "range": "+/- 0.184",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1460.298,
            "range": "+/- 9.642",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 29.997,
            "range": "+/- 0.27",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1463.701,
            "range": "+/- 6.56",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 14058.936,
            "range": "+/- 91.47",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 701.946,
            "range": "+/- 1.91",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 653886.787,
            "range": "+/- 3469.223",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10107.868,
            "range": "+/- 22.953",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 231.07,
            "range": "+/- 1.084",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2584.139,
            "range": "+/- 7.068",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 109.882,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 40980.3,
            "range": "+/- 185.915",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1895.988,
            "range": "+/- 8.745",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1621343.554,
            "range": "+/- 7921.02",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 26581.278,
            "range": "+/- 120.29",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 670.424,
            "range": "+/- 2.782",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6730.177,
            "range": "+/- 34.332",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 255.026,
            "range": "+/- 0.922",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 101067.825,
            "range": "+/- 371.66",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6219.871,
            "range": "+/- 12.49",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7020.26,
            "range": "+/- 38.675",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "3d7f01a4a3b7a33fcf297424098620e969a2548d",
          "message": "deps: update hashicorp/helm requirement from ~> 2.0 to ~> 3.2 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/52/commits/3d7f01a4a3b7a33fcf297424098620e969a2548d"
        },
        "date": 1788308723645,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 542.152,
            "range": "+/- 1.864",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3478.598,
            "range": "+/- 10.53",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 406.531,
            "range": "+/- 1.534",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1038.379,
            "range": "+/- 15.404",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 424.847,
            "range": "+/- 8.77",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12539.236,
            "range": "+/- 34.828",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 617.542,
            "range": "+/- 1.738",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3848.926,
            "range": "+/- 22.466",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 486.635,
            "range": "+/- 3.22",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1177.62,
            "range": "+/- 3.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.6,
            "range": "+/- 2.104",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13669.417,
            "range": "+/- 95.242",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2767.645,
            "range": "+/- 22.058",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.677,
            "range": "+/- 0.484",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.345,
            "range": "+/- 0.566",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1210.294,
            "range": "+/- 5.851",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1986.08,
            "range": "+/- 18.89",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 664.735,
            "range": "+/- 2.694",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 847.918,
            "range": "+/- 2.94",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1055.679,
            "range": "+/- 1.192",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11990.297,
            "range": "+/- 4.998",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.98,
            "range": "+/- 0.962",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3245.729,
            "range": "+/- 3.775",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 376.124,
            "range": "+/- 2.795",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78803.61,
            "range": "+/- 839.821",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12190.849,
            "range": "+/- 236.045",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 408.209,
            "range": "+/- 2.281",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.658,
            "range": "+/- 0.168",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 102.876,
            "range": "+/- 0.374",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.161,
            "range": "+/- 0.18",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1702.014,
            "range": "+/- 14.559",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.344,
            "range": "+/- 0.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2166.819,
            "range": "+/- 33.646",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17722.104,
            "range": "+/- 348.95",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 842.712,
            "range": "+/- 1.279",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 749971.465,
            "range": "+/- 1071.608",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11801.892,
            "range": "+/- 13.403",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.249,
            "range": "+/- 0.471",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3031.94,
            "range": "+/- 2.006",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 141.647,
            "range": "+/- 0.402",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46896.391,
            "range": "+/- 37.483",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2788.065,
            "range": "+/- 42.685",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2264582.091,
            "range": "+/- 11931.158",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35941.016,
            "range": "+/- 238.755",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 895.557,
            "range": "+/- 1.925",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9514.549,
            "range": "+/- 82.266",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 330.581,
            "range": "+/- 1.311",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 159064.536,
            "range": "+/- 2134.945",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7215.845,
            "range": "+/- 88.245",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8020.381,
            "range": "+/- 113.767",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "ea980d4c5125f22c0ee66dfb65d040648154f30e",
          "message": "fix(ci): the baseline comparison failed builds on measurement noise",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/76/commits/ea980d4c5125f22c0ee66dfb65d040648154f30e"
        },
        "date": 1788309073232,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 513.3,
            "range": "+/- 2.438",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3400.045,
            "range": "+/- 12.557",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 391.267,
            "range": "+/- 1.632",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 997.388,
            "range": "+/- 3.871",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 356.282,
            "range": "+/- 1.108",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12828.723,
            "range": "+/- 44.848",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 583.637,
            "range": "+/- 4.2",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6140.536,
            "range": "+/- 23.437",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 454.445,
            "range": "+/- 1.944",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1131.685,
            "range": "+/- 3.975",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 421.908,
            "range": "+/- 1.821",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13537.998,
            "range": "+/- 83.069",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2574.057,
            "range": "+/- 26.119",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 112.386,
            "range": "+/- 2.275",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 125.788,
            "range": "+/- 1.631",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1092.42,
            "range": "+/- 2.878",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1793.936,
            "range": "+/- 5.419",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 582.371,
            "range": "+/- 1.21",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 749.851,
            "range": "+/- 1.387",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1037.678,
            "range": "+/- 18.052",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10655.875,
            "range": "+/- 19.819",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 444.868,
            "range": "+/- 2.021",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2899.267,
            "range": "+/- 2.82",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 317.054,
            "range": "+/- 0.766",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73401.209,
            "range": "+/- 778.782",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 14537.319,
            "range": "+/- 318.153",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 360.242,
            "range": "+/- 0.769",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.98,
            "range": "+/- 0.135",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.693,
            "range": "+/- 0.282",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.839,
            "range": "+/- 0.106",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1482.642,
            "range": "+/- 9.717",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.598,
            "range": "+/- 0.153",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1934.989,
            "range": "+/- 5.329",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18153.314,
            "range": "+/- 296.129",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.134,
            "range": "+/- 0.661",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662327.473,
            "range": "+/- 241.582",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10433.499,
            "range": "+/- 6.391",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 250.029,
            "range": "+/- 0.807",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2690.697,
            "range": "+/- 4.456",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 128.305,
            "range": "+/- 0.65",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41548.428,
            "range": "+/- 93.85",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2258.362,
            "range": "+/- 10.409",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1983829.615,
            "range": "+/- 5870.073",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31484.821,
            "range": "+/- 93.507",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 797.646,
            "range": "+/- 3.173",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8158.915,
            "range": "+/- 44.44",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.97,
            "range": "+/- 1.781",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124019.088,
            "range": "+/- 211",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7430.232,
            "range": "+/- 81.005",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8506.688,
            "range": "+/- 114.159",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "70065f101d5e31d3de464a23bf8b01c88affc3ed",
          "message": "deps: update hashicorp/kubernetes requirement from ~> 2.0 to ~> 3.2 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/54/commits/70065f101d5e31d3de464a23bf8b01c88affc3ed"
        },
        "date": 1788364098576,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.874,
            "range": "+/- 0.54",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2694.167,
            "range": "+/- 8.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 317.444,
            "range": "+/- 0.774",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 774.64,
            "range": "+/- 2.447",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 287.841,
            "range": "+/- 0.562",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9863.438,
            "range": "+/- 61.258",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 482.773,
            "range": "+/- 1.423",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2938.234,
            "range": "+/- 4.998",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 375.29,
            "range": "+/- 0.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 921.817,
            "range": "+/- 1.774",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 346.797,
            "range": "+/- 0.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10430.608,
            "range": "+/- 29.345",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2073.695,
            "range": "+/- 4.25",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 91.419,
            "range": "+/- 0.476",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 91.036,
            "range": "+/- 0.48",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 911.748,
            "range": "+/- 2.085",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1467.88,
            "range": "+/- 3.623",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 513.157,
            "range": "+/- 2.006",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 702.147,
            "range": "+/- 24.839",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 835.987,
            "range": "+/- 3.069",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9321.913,
            "range": "+/- 9.728",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 389.069,
            "range": "+/- 0.925",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2518.1,
            "range": "+/- 2.228",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 278.715,
            "range": "+/- 0.627",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 57451.149,
            "range": "+/- 139.469",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8787.668,
            "range": "+/- 60.949",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 306.596,
            "range": "+/- 0.87",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.149,
            "range": "+/- 0.142",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 83.019,
            "range": "+/- 0.272",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.043,
            "range": "+/- 0.063",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1227.191,
            "range": "+/- 3.05",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 26.874,
            "range": "+/- 0.048",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1555.15,
            "range": "+/- 4.514",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11796.365,
            "range": "+/- 33.091",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 649.564,
            "range": "+/- 0.375",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 578831.331,
            "range": "+/- 250.864",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9117.587,
            "range": "+/- 2.057",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 216.174,
            "range": "+/- 0.39",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2344.981,
            "range": "+/- 2.316",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 107.544,
            "range": "+/- 0.158",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36205.817,
            "range": "+/- 12.282",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2021.958,
            "range": "+/- 34.187",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1746331.655,
            "range": "+/- 10241.572",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27446.492,
            "range": "+/- 108.49",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 688.68,
            "range": "+/- 1.159",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7203.958,
            "range": "+/- 85.806",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.571,
            "range": "+/- 0.704",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 108733.218,
            "range": "+/- 375.445",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5311.102,
            "range": "+/- 58.811",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5927.8,
            "range": "+/- 31.531",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "0c3ff7478f6e9abe1992e2da3716d45ac0ef30f9",
          "message": "deps(deps): bump wasmtime from 24.0.12 to 48.0.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/63/commits/0c3ff7478f6e9abe1992e2da3716d45ac0ef30f9"
        },
        "date": 1788364171922,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.351,
            "range": "+/- 1.556",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2602.413,
            "range": "+/- 14.314",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 326.177,
            "range": "+/- 0.588",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 777.306,
            "range": "+/- 2.293",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 288.05,
            "range": "+/- 0.675",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9828.048,
            "range": "+/- 31.696",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 477.428,
            "range": "+/- 1.066",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2970.399,
            "range": "+/- 8.017",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 368.315,
            "range": "+/- 0.634",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 909.083,
            "range": "+/- 4.189",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 337.131,
            "range": "+/- 1.248",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10441.21,
            "range": "+/- 41.337",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2044.684,
            "range": "+/- 7.56",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 79.094,
            "range": "+/- 0.428",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 92.576,
            "range": "+/- 0.964",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 927.664,
            "range": "+/- 2.37",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1507.457,
            "range": "+/- 1.447",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 497.993,
            "range": "+/- 1.532",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 643.641,
            "range": "+/- 2.346",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 816.485,
            "range": "+/- 0.782",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9306.357,
            "range": "+/- 17.187",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 404.635,
            "range": "+/- 4.743",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2512.602,
            "range": "+/- 1.765",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 283.202,
            "range": "+/- 1.047",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 58105.124,
            "range": "+/- 154.635",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8727.318,
            "range": "+/- 71.458",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 304.102,
            "range": "+/- 0.709",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 12.936,
            "range": "+/- 0.048",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 77.973,
            "range": "+/- 0.108",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.029,
            "range": "+/- 0.059",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1206.928,
            "range": "+/- 1.411",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 27.088,
            "range": "+/- 0.08",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1687.563,
            "range": "+/- 6.079",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11943.462,
            "range": "+/- 52.563",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 667.88,
            "range": "+/- 0.902",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579011.205,
            "range": "+/- 325.682",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9133.965,
            "range": "+/- 5.309",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 230.94,
            "range": "+/- 0.205",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2357.887,
            "range": "+/- 1.694",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 128.187,
            "range": "+/- 0.282",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36292.052,
            "range": "+/- 35.918",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1962.972,
            "range": "+/- 7.893",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1916682.057,
            "range": "+/- 26296.96",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27604.223,
            "range": "+/- 190.052",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 696.398,
            "range": "+/- 2.59",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7028.472,
            "range": "+/- 33.178",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.228,
            "range": "+/- 0.623",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124100.815,
            "range": "+/- 1762.517",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5206.023,
            "range": "+/- 20.434",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5828.147,
            "range": "+/- 17.836",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "058afc575248c0f41c9a9cf6a066ad5e2df6b74f",
          "message": "deps(deps): bump wgpu from 22.1.0 to 30.0.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/46/commits/058afc575248c0f41c9a9cf6a066ad5e2df6b74f"
        },
        "date": 1788364262777,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 556.407,
            "range": "+/- 1.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3358.453,
            "range": "+/- 10.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 425.07,
            "range": "+/- 1.882",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1034.018,
            "range": "+/- 8.003",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 390.775,
            "range": "+/- 1.66",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12664.131,
            "range": "+/- 51.602",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 631.998,
            "range": "+/- 1.733",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3805.186,
            "range": "+/- 14.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 489.948,
            "range": "+/- 2.288",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1194.162,
            "range": "+/- 7.563",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 448.752,
            "range": "+/- 1.495",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11944.737,
            "range": "+/- 61.293",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2781.878,
            "range": "+/- 12.715",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.844,
            "range": "+/- 0.26",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.647,
            "range": "+/- 0.614",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1206.578,
            "range": "+/- 6.659",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1907.492,
            "range": "+/- 5.491",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.857,
            "range": "+/- 1.243",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 819.633,
            "range": "+/- 1.079",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1071.028,
            "range": "+/- 1.995",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12245.079,
            "range": "+/- 110.751",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 512.916,
            "range": "+/- 2.098",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3269.268,
            "range": "+/- 3.965",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 371.978,
            "range": "+/- 1.665",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76188.188,
            "range": "+/- 352.645",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11099.015,
            "range": "+/- 45.473",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 399.808,
            "range": "+/- 0.902",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.859,
            "range": "+/- 0.092",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.639,
            "range": "+/- 0.254",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.183,
            "range": "+/- 0.103",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1595.752,
            "range": "+/- 2.524",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.528,
            "range": "+/- 0.087",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2149.261,
            "range": "+/- 15.133",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16248.162,
            "range": "+/- 175.67",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.917,
            "range": "+/- 0.322",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 754824.735,
            "range": "+/- 1189.645",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11816.497,
            "range": "+/- 8.282",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 284.228,
            "range": "+/- 0.722",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3035.095,
            "range": "+/- 2.668",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 144.814,
            "range": "+/- 0.318",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46885.476,
            "range": "+/- 23.078",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2535.373,
            "range": "+/- 8.677",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2422537.955,
            "range": "+/- 25633.514",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35257.329,
            "range": "+/- 119.262",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 915.61,
            "range": "+/- 10.932",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9261.308,
            "range": "+/- 36.399",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 334.733,
            "range": "+/- 1.975",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143297.067,
            "range": "+/- 928.947",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6874.814,
            "range": "+/- 43.358",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7752.163,
            "range": "+/- 68.093",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "07924eb74cb944b63c5152804e5fd42052bb22a7",
          "message": "deps(deps): bump spin from 0.9.9 to 0.12.3",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/64/commits/07924eb74cb944b63c5152804e5fd42052bb22a7"
        },
        "date": 1788364443127,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 637.996,
            "range": "+/- 1.269",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3580.364,
            "range": "+/- 17.679",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 521.734,
            "range": "+/- 3.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1156.816,
            "range": "+/- 8.069",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 474.118,
            "range": "+/- 1.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12518.656,
            "range": "+/- 34.493",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 672.175,
            "range": "+/- 2.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4059.58,
            "range": "+/- 33.627",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 542.967,
            "range": "+/- 4.334",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1227.624,
            "range": "+/- 3.999",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 497.78,
            "range": "+/- 1.036",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13712.832,
            "range": "+/- 70.037",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2847.199,
            "range": "+/- 21.235",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.867,
            "range": "+/- 0.356",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.905,
            "range": "+/- 0.552",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1252.624,
            "range": "+/- 1.998",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1963.168,
            "range": "+/- 4.153",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 698.037,
            "range": "+/- 0.943",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 880.435,
            "range": "+/- 1.075",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1055.923,
            "range": "+/- 0.961",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11979.425,
            "range": "+/- 6.16",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.16,
            "range": "+/- 1.067",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3243.077,
            "range": "+/- 1.811",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 361.4,
            "range": "+/- 0.922",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 93769.042,
            "range": "+/- 1235.922",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11220.444,
            "range": "+/- 69.629",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 395.985,
            "range": "+/- 0.892",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.09,
            "range": "+/- 0.141",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.103,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.946,
            "range": "+/- 0.109",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1566.421,
            "range": "+/- 2.008",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.762,
            "range": "+/- 0.167",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2365.948,
            "range": "+/- 29.355",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16197.186,
            "range": "+/- 180.024",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.621,
            "range": "+/- 0.95",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 750269.151,
            "range": "+/- 481.423",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11831.518,
            "range": "+/- 14.311",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 290.574,
            "range": "+/- 0.245",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3074.275,
            "range": "+/- 5.657",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 150.952,
            "range": "+/- 0.357",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 48210.22,
            "range": "+/- 248.911",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2600.507,
            "range": "+/- 21.076",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2281666.261,
            "range": "+/- 16548.123",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35315.685,
            "range": "+/- 150.457",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 949.235,
            "range": "+/- 20.801",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9058.773,
            "range": "+/- 19.997",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 333.51,
            "range": "+/- 3.554",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140309.744,
            "range": "+/- 1090.207",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7476.995,
            "range": "+/- 118.792",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8157.355,
            "range": "+/- 116.894",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "4000913c9cf31d63049a94ababa171bff0ffcb72",
          "message": "deps(deps): bump windows-sys from 0.59.0 to 0.61.2",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/50/commits/4000913c9cf31d63049a94ababa171bff0ffcb72"
        },
        "date": 1788364553057,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 534.809,
            "range": "+/- 2.736",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3383.692,
            "range": "+/- 7.804",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 396.306,
            "range": "+/- 0.838",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.359,
            "range": "+/- 2.807",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 372.633,
            "range": "+/- 2.56",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 14913.246,
            "range": "+/- 76.048",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 614.792,
            "range": "+/- 1.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3788.733,
            "range": "+/- 14.358",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 475.03,
            "range": "+/- 1.623",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1181.241,
            "range": "+/- 3.327",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 437.738,
            "range": "+/- 0.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 17142.986,
            "range": "+/- 44.584",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2823.09,
            "range": "+/- 10.208",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.671,
            "range": "+/- 0.371",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.249,
            "range": "+/- 0.493",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1257.187,
            "range": "+/- 5.359",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2048.233,
            "range": "+/- 14.443",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 668.481,
            "range": "+/- 1.942",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 861.16,
            "range": "+/- 2.612",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1056.551,
            "range": "+/- 0.754",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12006.303,
            "range": "+/- 8.443",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 499.303,
            "range": "+/- 0.382",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3243.232,
            "range": "+/- 1.608",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.162,
            "range": "+/- 0.939",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 393.952,
            "range": "+/- 1.408",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.703,
            "range": "+/- 0.183",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.334,
            "range": "+/- 0.513",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.997,
            "range": "+/- 0.238",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1573.808,
            "range": "+/- 3.737",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.412,
            "range": "+/- 0.238",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.699,
            "range": "+/- 1.242",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747482.449,
            "range": "+/- 403.216",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11809.61,
            "range": "+/- 6.247",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.844,
            "range": "+/- 0.549",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3030.015,
            "range": "+/- 2.001",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.481,
            "range": "+/- 0.393",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46774.364,
            "range": "+/- 22.734",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2706.232,
            "range": "+/- 50.111",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2229316.087,
            "range": "+/- 4555.331",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35191.829,
            "range": "+/- 106.918",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 894.251,
            "range": "+/- 3.104",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9106.068,
            "range": "+/- 52.117",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.082,
            "range": "+/- 1.203",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140253.44,
            "range": "+/- 694.66",
            "unit": "ns"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "62969029e16efd2913d1795dcbf0dc3f8ca7ee04",
          "message": "deps(deps): bump toml from 0.8.23 to 1.1.4+spec-1.1.0",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/68/commits/62969029e16efd2913d1795dcbf0dc3f8ca7ee04"
        },
        "date": 1788364865653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 576.648,
            "range": "+/- 2.327",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3446.705,
            "range": "+/- 11.585",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 448.241,
            "range": "+/- 1.264",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1064.105,
            "range": "+/- 5.929",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 415.487,
            "range": "+/- 0.932",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12761.221,
            "range": "+/- 76.436",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 651.626,
            "range": "+/- 2.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3986.622,
            "range": "+/- 13.757",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 513.229,
            "range": "+/- 1.402",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1208.807,
            "range": "+/- 4.715",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 465.751,
            "range": "+/- 0.833",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13321.321,
            "range": "+/- 62.109",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2750.827,
            "range": "+/- 14.264",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.312,
            "range": "+/- 0.174",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 120.479,
            "range": "+/- 0.656",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1265.182,
            "range": "+/- 8.076",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1997.406,
            "range": "+/- 9.16",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 693.945,
            "range": "+/- 2.47",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 898.781,
            "range": "+/- 4.553",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1062.976,
            "range": "+/- 1.598",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12034.352,
            "range": "+/- 13.268",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 501.038,
            "range": "+/- 0.907",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3244.041,
            "range": "+/- 1.333",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.812,
            "range": "+/- 0.488",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75115.626,
            "range": "+/- 276.895",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11075.453,
            "range": "+/- 59.487",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 401.132,
            "range": "+/- 0.762",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.98,
            "range": "+/- 0.163",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.034,
            "range": "+/- 0.411",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.832,
            "range": "+/- 0.113",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1604.015,
            "range": "+/- 2.744",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.003,
            "range": "+/- 0.154",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1999.889,
            "range": "+/- 8.758",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15141.492,
            "range": "+/- 81.182",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 865.373,
            "range": "+/- 0.646",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747080.839,
            "range": "+/- 242.719",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11784.396,
            "range": "+/- 4.7",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 302.63,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3044.074,
            "range": "+/- 2.12",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 165.663,
            "range": "+/- 0.595",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46740.649,
            "range": "+/- 14.305",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2523.498,
            "range": "+/- 6.65",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2255197.909,
            "range": "+/- 7601.238",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35126.468,
            "range": "+/- 84.598",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 896.028,
            "range": "+/- 3.217",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9086.071,
            "range": "+/- 35.482",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.705,
            "range": "+/- 1.423",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140931.443,
            "range": "+/- 612.964",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6711.175,
            "range": "+/- 26.824",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7439.721,
            "range": "+/- 32.702",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}