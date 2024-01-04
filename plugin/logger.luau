local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local LogService = game:GetService("LogService")
local ScriptContext = game:GetService("ScriptContext")

local module = {
  queuedMessages = {},
  timeSinceLastSend = 0,
  messageSendRate = 0.1,
  sendMessages = false,
  serverId = "",
  serverUrl = "",
  heartbeatConnection = nil,
  logConnection = nil,
  errorConnection = nil,
  tryFlush = true,
}

local logTypeToLevel = {
	[Enum.MessageType.MessageOutput] = "Print",
	[Enum.MessageType.MessageInfo] = "Info",
	[Enum.MessageType.MessageWarning] = "Warning",
	[Enum.MessageType.MessageError] = "Error",
}

function module:flushMessages()
  if #self.queuedMessages == 0 then
    self.timeSinceLastSend = 0
    return
  end

	local encoded = HttpService:JSONEncode(self.queuedMessages)
	self.queuedMessages = {}
  local backoff = 1
  while self.tryFlush do
    local success, e = pcall(function()
      return HttpService:PostAsync(self.serverUrl .. "/messages", encoded)
    end)
    if success then
      break
    else
      print(e)
    end
    print(string.format('waiting %d seconds to flush logs ...', backoff))
    wait(backoff)
    backoff = backoff * 2
  end
	self.timeSinceLastSend = 0
end

function module:heartbeat(dt: number)
    if not self.sendMessages then
      return
    end
    self.timeSinceLastSend = self.timeSinceLastSend + dt

    if self.timeSinceLastSend >= self.messageSendRate then
      self:flushMessages()
    end
end

function module:handleLogEvent(body: string, messageType: Enum.MessageType)
  local level = logTypeToLevel[messageType] or "Info"
  if level == "Info" or level == "Print" or level == "Warning" then
    table.insert(self.queuedMessages, {
      type = "Output",
      level = level,
      server = self.serverId,
      body = body,
    })
  end
end

function module:errorHandler(message: string)
  local traceback = message
  table.insert(self.queuedMessages, {
    type = "Output",
    level = "Warning",
    server = self.serverId,
    body = "caught an error while calling your main method - the error message will be printed below:"
  })
  table.insert(self.queuedMessages, {
    type = "Output",
    level = "ScriptError",
    server = self.serverId,
    body = traceback,
  })
end

function module:handleScriptError(message: string, trace: string, script: Instance)
  local scriptName = script:GetFullName()
  local body = scriptName .. "\n" .. message .. trace
  table.insert(self.queuedMessages, {
    type = "Output",
    level = "Error",
    server = self.serverId,
    body = body,
  })
end

function module:init(serverId: string, serverUrl: string) 
  self.serverId = serverId
  self.serverUrl = serverUrl
  -- queue up any messages we may have missed
  for i, v in ipairs(LogService:GetLogHistory()) do
    local level = logTypeToLevel[v.messageType] or "Info"
    table.insert(self.queuedMessages, {
      type = "Output",
      level = level,
      server = self.serverId,
      body = v.message
    })
  end
  self.heartbeatConnection = RunService.Heartbeat:Connect(function(dt)
    module:heartbeat(dt)
  end)

  self.logConnection = LogService.MessageOut:Connect(function(body, messageType)
    module:handleLogEvent(body, messageType)
  end)

  self.errorConnection = ScriptContext.Error:Connect(function(message: string, trace: string, script: Instance)
    module:handleScriptError(message, trace, script)
  end)
end

function module:disconnect()
  self.heartbeatConnection:Disconnect()
  self.heartbeatConnection = nil
  self.logConnection:Disconnect()
  self.logConnection = nil
  self.errorConnection:Disconnect()
  self.errorConnection = nil
end

return module
