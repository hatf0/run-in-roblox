local fs = require("@lune/fs")
local roblox = require("@lune/roblox")

local plugin = roblox.Instance.new("Script")
plugin.Name = "run-in-roblox-plugin"
plugin.Source = fs.readFile("./plugin.luau")

local logger = roblox.Instance.new("ModuleScript")
logger.Parent = plugin
logger.Name = "Logger"
logger.Source = fs.readFile("./logger.luau")

local model = roblox.serializeModel({plugin})
fs.writeFile("./plugin.rbxm", model)