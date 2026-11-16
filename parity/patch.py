"""Copy the five decompiled sources that need a hook into patched/ and add the hook lines."""
import os, sys
SRC = sys.argv[1]
DST = sys.argv[2]
def patch(rel, edits):
    s = open(os.path.join(SRC, rel)).read()
    for old, new in edits:
        assert s.count(old) == 1, (rel, old, s.count(old))
        s = s.replace(old, new)
    out = os.path.join(DST, rel)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, "w").write(s)
patch("net/minecraft/client/Minecraft.java", [
    ("this.runTick();", "rig.Rig.clientPre(this); this.runTick(); rig.Rig.clientPost(this);"),
])
patch("net/minecraft/server/MinecraftServer.java", [
    ("        long i = System.nanoTime();\n        ++this.tickCounter;", "        long i = System.nanoTime();\n        rig.Rig.serverPre(this);\n        ++this.tickCounter;"),
    ('        this.theProfiler.startSection("tallying");', '        rig.Rig.serverPost(this);\n        this.theProfiler.startSection("tallying");'),
])
patch("net/minecraft/network/PacketThreadUtil.java", [
    ("p_180031_0_.processPacket(p_180031_1_);", "rig.Rig.onHandle(p_180031_0_, p_180031_1_); p_180031_0_.processPacket(p_180031_1_);"),
])
patch("net/minecraft/network/NetHandlerPlayServer.java", [
    ("    public void sendPacket(final Packet packetIn)\n    {", "    public void sendPacket(final Packet packetIn)\n    {\n        rig.Rig.onServerSend(this, packetIn);"),
])
patch("net/minecraft/client/network/NetHandlerPlayClient.java", [
    ("    public void addToSendQueue(Packet p_147297_1_)\n    {", "    public void addToSendQueue(Packet p_147297_1_)\n    {\n        rig.Rig.onClientSend(p_147297_1_);"),
    # The answer to a server teleport goes around addToSendQueue; note it down as well.
    ("this.netManager.sendPacket(new C03PacketPlayer.C06PacketPlayerPosLook(", "rig.Rig.onClientSend(null); this.netManager.sendPacket(rig.Rig.noted(new C03PacketPlayer.C06PacketPlayerPosLook("),
    ("entityplayer.rotationYaw, entityplayer.rotationPitch, false));", "entityplayer.rotationYaw, entityplayer.rotationPitch, false)));"),
])
print("patched")
