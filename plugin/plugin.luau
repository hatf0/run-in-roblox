local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local Logger = require(script:WaitForChild("Logger"))
local SERVER_URL = "http://localhost:7777" 
local SERVER_ID = HttpService:GenerateGUID(false)

if not RunService:IsStudio() then
  return
end

function waitForStart()
  local backoff = 1
  while true do
    local pingSuccess, e = pcall(function()
      return HttpService:GetAsync(string.format("%s%s", SERVER_URL, "/ping"))
    end)
    if pingSuccess then
      break
    else
      -- for some reason, we can only really tell if this is executing
      -- on the client by checking if we get this error, so infinite loop
      -- if we get it...
      if e == "Http requests can only be executed by game server" then
        while true do
        end
      end
      print(e)
    end
    print(string.format('waiting %d seconds for run-on-roblox service to come alive...', backoff))
    wait(backoff)
    backoff = backoff * 2
  end
end

function getStatusMessage()
  local success, message = pcall(function()
    return HttpService:GetAsync(string.format("%s?server=%s", SERVER_URL .. "/status", SERVER_ID))
  end)
  if not success then
    return false
  end

  local message = HttpService:JSONDecode(message)
  return message
end

waitForStart()

Logger:init(SERVER_ID, SERVER_URL)

-- register with the external coordinator
HttpService:PostAsync(SERVER_URL .. "/start", HttpService:JSONEncode({
	server = SERVER_ID
}))

function cleanup() 
  Logger:disconnect()
  HttpService:PostAsync(SERVER_URL .. "/stop", HttpService:JSONEncode({
    server = SERVER_ID
  }))
end

while true do
  local message = getStatusMessage()
  if not message.run then
    wait(2)
    cleanup()
    return
  end

  local script = Instance.new("ModuleScript")
  script.Source = message.src
  script.Parent = game:GetService("Workspace")

  local loadSuccess, messageOrMain = xpcall(require, debug.traceback, script)

  if not loadSuccess then
    local sacrificialEvent = Instance.new("BindableEvent")
    sacrificialEvent.Event:Connect(function()
      error('could not call `require` on your script - please make sure that it is a proper ModuleScript and returns a table')
    end)
    sacrificialEvent:Fire()
    wait(2)
    continue
  end

  if not messageOrMain.Main then
    local sacrificialEvent = Instance.new("BindableEvent")
    sacrificialEvent.Event:Connect(function()
      error('your script is missing a `Main` method - please define it')
    end)
    sacrificialEvent:Fire()
    wait(2)
    continue
  end

  local mainSuccess, message = xpcall(messageOrMain.Main, debug.traceback)

  if not mainSuccess then
    local sacrificialEvent = Instance.new("BindableEvent")
    sacrificialEvent.Event:Connect(function()
      print('caught an error while calling your main method - the error message will be printed below:')
      error(message, 0)
    end)
    sacrificialEvent:Fire()
    wait(2)
    continue
  else
    script.Parent = nil
    script:Destroy()
  end

  wait(15)
end



