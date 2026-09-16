# den.nvim

Status: active
Stack: rust, lua, neovim

## Outcome

The Lua plugin becomes a thin binding over the Rust engine, the way blink.cmp
loads its own binary.

## Next actions

- [ ] Sketch the lua_module surface @tag(rust) @id(plugin-surface) @status(doing) @order(1000000)
- [ ] Decide the fallback when the cdylib is missing @tag(rust) @id(plugin-fallback) @status(backlog) @order(2000000)
- [x] Confirm how blink.cmp loads its binary @tag(research) @id(plugin-blink) @status(done) @order(1000000)

## Notes

Keep the pure-Lua path working so a user without the binary is not stranded.
