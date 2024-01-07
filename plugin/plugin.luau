local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local Logger = require(script:WaitForChild("Logger"))
local SERVER_URL = "http://localhost:7777" 
local SERVER_ID = HttpService:GenerateGUID(false)

if not RunService:IsStudio() then
  return
end

function pingCoordinator() 
  return pcall(function()
    return HttpService:GetAsync(string.format("%s?server=%s", SERVER_URL .. "/ping", SERVER_ID))
  end)
end

function waitForStart()
  local backoff = 1
  while true do
    local pingSuccess, e = pcall(function()
      return HttpService:GetAsync(SERVER_URL .. "/")
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
    end
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

function getEvents()
  local success, events = pcall(function()
    return HttpService:GetAsync(string.format("%s?server=%s", SERVER_URL .. "/events", SERVER_ID))
  end)

  if not success then
    return nil
  end

  local events = HttpService:JSONDecode(events)
  return events
end

function init()
  Logger:init(SERVER_ID, SERVER_URL)
  waitForStart()
  -- register with the external coordinator
  local success, e = pcall(function()
    HttpService:PostAsync(SERVER_URL .. "/start", HttpService:JSONEncode({
      server = SERVER_ID
    }))
  end)
  if not success then
    warn("failed to register with backend service")
    warn(e)
    return false
  else
    Logger.sendMessages = true
    return true
  end
end

function reinit()
  waitForStart()
  -- register with the external coordinator
  local success, e = pcall(function()
    HttpService:PostAsync(SERVER_URL .. "/start", HttpService:JSONEncode({
      server = SERVER_ID
    }))
  end)
  if not success then
    warn("failed to register with backend service")
    warn(e)
    return false
  else
    Logger.queuedMessages = {}
    Logger.tryFlush = true
    Logger.sendMessages = true
    Logger:backfillMessages()
    Logger:connect()
    return true
  end
end

init()

function cleanup() 
  Logger:disconnect()
  pcall(function ()
    HttpService:PostAsync(SERVER_URL .. "/stop", HttpService:JSONEncode({
      server = SERVER_ID
    }))
  end)
end

function runScript(src)
  local script = Instance.new("ModuleScript")
  script.Name = "run-in-roblox-script"
  script.Source = src
  script.Parent = game:GetService("Workspace")

  local loadSuccess, messageOrMain = xpcall(require, debug.traceback, script)

  if not loadSuccess then
    local sacrificialEvent = Instance.new("BindableEvent")
    sacrificialEvent.Event:Connect(function()
      error('could not call `require` on your script - please make sure that it is a proper ModuleScript and returns a table')
    end)
    sacrificialEvent:Fire()
    Logger:waitForFlush()
    script.Parent = nil
    script:Destroy()
    return
  end

  if not messageOrMain.Main then
    local sacrificialEvent = Instance.new("BindableEvent")
    sacrificialEvent.Event:Connect(function()
      error('your script is missing a `Main` method - please define it')
    end)
    sacrificialEvent:Fire()
    Logger:waitForFlush()
    script.Parent = nil
    script:Destroy()
    return
  end

  local success, retVal = xpcall(messageOrMain.Main, debug.traceback)
  if not success then
    Logger:errorHandler(retVal)
  end

  script.Parent = nil
  script:Destroy()
end

task.spawn(function()
  while true do
    local success, e = pingCoordinator()
    if not success then
      warn('backend server is no longer alive - reinitializing')
      Logger.tryFlush = false
      Logger.sendMessages = false
      while true do
        local success = reinit()
        if success then
          break
        end
      end
    end

    local events = getEvents()
    if not events then 
      warn("could not get events")
      Logger:waitForFlush()
      cleanup()
      return
    end

    if #events ~= 0 then
      for _, v in ipairs(events) do
        if v.RunScript then
          runScript(v.RunScript.script)
          if v.RunScript.oneshot then
            Logger:waitForFlush()
            cleanup()
            return
          end
        end
        if v == "Deregister" then
          Logger:waitForFlush()
          cleanup()
          SERVER_ID = HttpService:GenerateGUID(false)
          Logger.serverId = SERVER_ID
        end
      end
    end
    wait(1)
  end
end)

