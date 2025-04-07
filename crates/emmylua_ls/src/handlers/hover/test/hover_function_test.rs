#[cfg(test)]
mod tests {

    use crate::handlers::hover::test::{HoverVirtualWorkspace, VirtualHoverResult};

    #[test]
    fn test_1() {
        let mut ws = HoverVirtualWorkspace::new();
        assert!(ws.check_hover(
            r#"
                ---@param a number 参数a
                ---@return number a 返回值a
                local function delete(a)
                end

                local delete2 = delete
                local delete3 = delete2
                local <??>delete4 = delete3
            "#,
            VirtualHoverResult {
                value: "\n```lua\nlocal function delete(a: number)\n  -> a: number\n\n```\n\n---\n\n@*param* `a` — 参数a\n\n\n\n@*return* `a`  — 返回值a\n\n\n".to_string(),
            },
        ));

        assert!(ws.check_hover(
            r#"
                ---@param a number 参数a
                ---@return number a 返回值a
                local function delete(a)
                end

                local delete2 = delete
                local delete3 = delete2
                local delete4 = delete3
                local deleteObj = {
                    <??>aaa = delete4
                }
            "#,
            VirtualHoverResult {
                value: "\n```lua\nlocal function delete(a: number)\n  -> a: number\n\n```\n\n---\n\n@*param* `a` — 参数a\n\n\n\n@*return* `a`  — 返回值a\n\n\n".to_string(),
            },
        ));

        assert!(ws.check_hover(
            r#"
                ---@param a number 参数a
                ---@return number a 返回值a
                local function delete(a)
                end

                local delete2 = delete
                local delete3 = delete2
                local delete4 = delete3
                local deleteObj = {
                    aa = delete4
                }

                local deleteObj2 = {
                    <??>aa = deleteObj.aa
                }
            "#,
            VirtualHoverResult {
                value: "\n```lua\nlocal function delete(a: number)\n  -> a: number\n\n```\n\n---\n\n@*param* `a` — 参数a\n\n\n\n@*return* `a`  — 返回值a\n\n\n".to_string(),
            },
        ));
    }

    #[test]
    fn test_2() {
        let mut ws = HoverVirtualWorkspace::new();
        ws.def(
            r#"
            ---@class Game
            ---@field event fun(self: self, owner: "abc"): any 测试1
            ---@field event fun(self: self, owner: "def"): any 测试2
            local Game = {}

            ---说明
            ---@param key string 参数key
            ---@param value string 参数value
            ---@return number ret @返回值
            function Game:add(key, value)
                self.aaa = 1
            end
            "#,
        );

        assert!(ws.check_hover(
            r#"


            ---@type Game
            local game

            local local_a = game.add
            local <??>local_b = local_a
            "#,
            VirtualHoverResult {
                value: "\n```lua\n(method) Game:add(key: string, value: string)\n  -> ret: number\n\n```\n\n---\n\n说明\n\n@*param* `key` — 参数key\n\n@*param* `value` — 参数value\n\n\n\n@*return* `ret`  — 返回值\n\n\n" .to_string(),
            },
        ));
    }

    #[test]
    fn test_3() {
        let mut ws = HoverVirtualWorkspace::new();
        ws.def(
            r#"
            ---@class Hover.Test3<T>
            ---@field event fun(self: self, event: "A", key: T)
            ---@field event fun(self: self, event: "B", key: T)
            local Test3 = {}
            "#,
        );

        assert!(ws.check_hover(
            r#"
                ---@type Hover.Test3<string>
                local test3

                local <??>event = test3.event
            "#,
            VirtualHoverResult {
                value: "\n```lua\n(method) Test3:event(event: \"B\", key: string)\n```\n\n&nbsp;&nbsp;in class `Hover.Test3`\n\n---\n\n---\n\n```lua\n(method) Test3:event(event: \"A\", key: string)\n```\n".to_string(),
            },
        ));
    }
}
