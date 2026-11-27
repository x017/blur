-- Sample Blur Lua extension

local recent_files = {}

blur.log("hello-blur extension loaded")

blur.register_command{
    id = "hello-blur.say-hello",
    title = "Hello Blur: Say Hello",
    category = "Hello Blur",
    run = function(ctx)
        local target = ctx.active_path or ctx.workspace_root or "Blur"
        blur.notify("Hello from Lua! Active context: " .. target, "info")
    end,
}

blur.bind("ctrl-alt-h", "hello-blur.say-hello")

blur.register_panel{
    id = "hello-blur.panel",
    title = "Hello Blur",
    render = function()
        local items = {
            {
                label = "Say Hello",
                detail = "Run the sample greeting command",
                command = "hello-blur.say-hello",
            },
        }
        for i = #recent_files, 1, -1 do
            table.insert(items, {
                label = "Opened: " .. recent_files[i],
                detail = "Captured via blur.on('file_opened')",
            })
        end
        return {
            title = "Hello Blur",
            items = items,
        }
    end,
}

blur.on("file_opened", function(payload)
    if type(payload) == "table" and type(payload.path) == "string" then
        table.insert(recent_files, payload.path)
        if #recent_files > 10 then
            table.remove(recent_files, 1)
        end
    end
end)
