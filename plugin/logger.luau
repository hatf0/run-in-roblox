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
  tryingToFlush = false,
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

  if self.tryingToFlush then
    return
  end

	local encoded = HttpService:JSONEncode(self.queuedMessages)
  self.tryingToFlush = true
	self.queuedMessages = {}
  local backoff = 1
  while self.tryFlush do
    local success, _ = pcall(function()
      return HttpService:PostAsync(self.serverUrl .. "/messages", encoded)
    end)
    if success then
      break
    end
    wait(backoff)
    backoff = backoff * 2
  end
  self.tryingToFlush = false
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

function module:backfillMessages()
  for i, v in ipairs(LogService:GetLogHistory()) do
    local level = logTypeToLevel[v.messageType] or "Info"
    table.insert(self.queuedMessages, {
      type = "Output",
      level = level,
      server = self.serverId,
      body = v.message
    })
  end
end

function module:init(serverId: string, serverUrl: string) 
  self.serverId = serverId
  self.serverUrl = serverUrl
  -- queue up any messages we may have missed
  self:backfillMessages()
  self:connect()
end

function module:connect()
  self:disconnect()
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

function module:waitForFlush() 
  if not self.sendMessages then
    return
  end
  
  if not self.tryFlush then
    return
  end

  while #self.queuedMessages ~= 0 do
      wait(1)
  end
end

function module:disconnect()
  if self.heartbeatConnection then
    self.heartbeatConnection:Disconnect()
    self.heartbeatConnection = nil
  end
  if self.logConnection then
    self.logConnection:Disconnect()
    self.logConnection = nil
  end
  if self.errorConnection then
    self.errorConnection:Disconnect()
    self.errorConnection = nil
  end
end

return module
