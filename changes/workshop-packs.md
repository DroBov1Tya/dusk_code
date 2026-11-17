Added
The Workshop installs skills, skill packs and agent packs, not only plugins: one package for any system, unpacked under the plugin rules into `skills/` or `agents/`; updates and removal keep files you edited. Skill packs, the built-in skills as `duskcode` among them, can be switched off whole (`skill_packs:` in `config.yml`). Two built-in skills are new, `computer-use` and `token-thrift`; the other skills and most agents moved to Workshop packs, and copies earlier versions installed stay as your own.

A Workshop pack must name the skills and agents it links to by path (`duskcode-agents/backend-engineer`); an ignored test (`DUSKCODE_WORKSHOP_DIST`) installs the built packs and checks every link resolves.
