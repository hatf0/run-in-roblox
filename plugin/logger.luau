local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local LogService = game:GetService("LogService")

local module = {
  queuedMessages = {},
  timeSinceLastSend = 0,
  messageSendRate = 0.1,
  serverId = "",
  serverUrl = "",
  heartbeatConnection = nil,
  logConnection = nil,
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
  while true do
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
    self.timeSinceLastSend = self.timeSinceLastSend + dt

    if self.timeSinceLastSend >= self.messageSendRate then
      self:flushMessages()
    end
end

function module:handleLogEvent(body: string, messageType: Enum.MessageType)
  table.insert(self.queuedMessages, {
    type = "Output",
    level = logTypeToLevel[messageType] or "Info",
    server = self.serverId,
    body = body,
  })
end

function module:init(serverId: string, serverUrl: string) 
  self.serverId = serverId
  self.serverUrl = serverUrl
  self.heartbeatConnection = RunService.Heartbeat:Connect(function(dt)
    module:heartbeat(dt)
  end)

  self.logConnection = LogService.MessageOut:Connect(function(body, messageType)
    module:handleLogEvent(body, messageType)
  end)
end

function module:disconnect()
  self.heartbeatConnection:Disconnect()
  self.heartbeatConnection = nil
  self.logConnection:Disconnect()
  self.logConnection = nil
end

return module
