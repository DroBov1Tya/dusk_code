# Retired built-in skill files

Move a listed file out of active skills only when its SHA-256 matches the former bundled bytes at inspection time.
Keep all moved contents in private `retired-skills/<id>/skills/...` recovery storage under the rules root; do not delete them.
This preserves concurrent edits made between inspection and rename. Files already modified at inspection and files added by users stay in place.

| Path relative to rules | SHA-256 |
|---|---|
| skills/frontend-design/LICENSE.txt | 0d542e0c8804e39aa7f37eb00da5a762149dc682d7829451287e11b938e94594 |
| skills/gh-address-comments/LICENSE.txt | 58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd |
| skills/gh-address-comments/scripts/fetch_comments.py | d7bc64f6b26f7482f9ca9dd7c923a5b8b20e1ef13491ae2d9052cd208a7c1ad1 |
| skills/gh-fix-ci/LICENSE.txt | 4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9 |
| skills/gh-fix-ci/scripts/inspect_pr_checks.py | 9459e5b03f86785d184a83e5cd8d621b832e918ec04158a2962d867785812c6c |
| skills/jupyter-notebook/LICENSE.txt | 4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9 |
| skills/jupyter-notebook/assets/experiment-template.ipynb | 25f85914103cd84244e20ef378f59097479c72481cd18d3a931354d107c3f300 |
| skills/jupyter-notebook/assets/tutorial-template.ipynb | 704a809c09c7c9e6e5c437860df95524412b50c58571a14204610d73e0744765 |
| skills/jupyter-notebook/scripts/new_notebook.py | a004e2ea3115edb24313928be0be6d1dc07e571d7ca6c1b24375f0569c616d2f |
| skills/mcp-builder/LICENSE.txt | bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362 |
| skills/mcp-builder/scripts/connections.py | 9403668a2041568772082a8b334122c1f88daf0541fb393af4522d0094a47a6e |
| skills/mcp-builder/scripts/evaluation.py | 49ed1d17cdce5da101b210197740713f49b935c29d4f339542a14b132658e6f7 |
| skills/mcp-builder/scripts/example_evaluation.xml | 9272b348ddcc4b06ba562367ccd0770e018158c0068ac5116d5e34aaeff8777a |
| skills/mcp-builder/scripts/requirements.txt | d5d7558b2368ecea9dfeed7d1fbc71ee9e0750bebd1282faa527d528a344c3c7 |
| skills/playwright/LICENSE.txt | 9a7110fc2d2f964038e5dc49128f908f29f47a574c961cba16085914e879cbda |
| skills/playwright/NOTICE.txt | 417d5d3372914ecfd9fe102ac1d7dc2119253209f9718d6c222f616876c1904b |
| skills/playwright/scripts/playwright_cli.sh | aa3fdff5d0e4556177f4dfd5f04117e772aa54f94b6a2e34b6c0edf629c6b9b5 |
| skills/security-best-practices/LICENSE.txt | 4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9 |
| skills/security-threat-model/LICENSE.txt | 4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9 |
| skills/skill-creator/LICENSE.txt | bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362 |
| skills/skill-creator/agents/analyzer.md | bf68f4cac5a56c673a928c2e6d619586c5b93ea364026ab37547772cb45a663a |
| skills/skill-creator/agents/comparator.md | fe1fc9787c495d864c5d6eada47396478572325fde1b33a96d78bf4b849b7a3e |
| skills/skill-creator/agents/grader.md | 57134da0c1a4eea33fbd74a1c9c44aa814f07d6bc64de303edb586f941e5d21a |
| skills/skill-creator/assets/eval_review.md | 1c07e65eecc07e63bac14a5a2f8e963fde1dd1bad27c5cc16eb80821fa61f9d2 |
| skills/skill-creator/eval-viewer/generate_review.py | 38c8018e6df847d4d6a5b342a6d74d3c5d17bfcb3e2e838941434838422d126b |
| skills/skill-creator/references/schemas.md | 3f35325f9292c32bc04ff3eeb340a59cd4d9143ba395337f03c883ee400c2d84 |
| skills/skill-creator/scripts/__init__.py | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| skills/skill-creator/scripts/aggregate_benchmark.py | 6d1644d65d556bc01cce8f2c95af56929f77b2dd9ee37c5c9cbed021c6224ef6 |
| skills/skill-creator/scripts/generate_report.py | 7cb37323b521f3479d86c9dcc83e73580f9198d9d5a9cd2e726b64c9384b22f8 |
| skills/skill-creator/scripts/improve_description.py | 87d864570220b699fac52da309d2d6efdb060647bfebc74f768128e646accf80 |
| skills/skill-creator/scripts/package_skill.py | 1a33059b0db1ef73375d46d513e5ea81369d2e8838c970597b0d52ddef8d1c0f |
| skills/skill-creator/scripts/quick_validate.py | 67cf5703402013936c8fb75ad6a1afecd8841d45cc5e606b634eb05825fde365 |
| skills/skill-creator/scripts/run_eval.py | 43e3b8f80dbf69c343967ba77e268fae991d9fa3ed68b32a0ff02532cd48657f |
| skills/skill-creator/scripts/run_loop.py | 648d1e0dccd9f919b43c54bb4caf119c5f75cf495df71369fba5f27880f21d09 |
| skills/skill-creator/scripts/utils.py | 3af8ae62c40c73ab712207436a0d9a981e845f25c5a7040229eb189cc8e45bb1 |
| skills/spike/LICENSE.txt | f27db279e3df709293a13070ccfe12e63cd0bc579be9d4fe3bde57d7c9bd5868 |
| skills/systematic-debugging/LICENSE.txt | 833ebec48ae8af8dd890d4ba9ce95880cf1235e1063ac0fc12237777829032be |
| skills/test-driven-development/LICENSE.txt | 833ebec48ae8af8dd890d4ba9ce95880cf1235e1063ac0fc12237777829032be |
| skills/webapp-testing/LICENSE.txt | bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362 |
| skills/webapp-testing/examples/console_logging.py | ea46877289acb82da7e7ce59d0bc37c8977cd57e2a006d0c88d7a1c625bf95da |
| skills/webapp-testing/examples/element_discovery.py | d63c89604a22f8845d724e95dda45db49b1bf57c25ce0a83afbb7b8da3d402f0 |
| skills/webapp-testing/examples/static_html_automation.py | 9d533aafb875ee3ab8b8ebf8f5b9003ac8d999da3d09b285cce252e623140064 |
| skills/webapp-testing/scripts/with_server.py | b0dcf4918935b795f4eda9821579b9902119235ff4447f687a30286e7d0925fd |
