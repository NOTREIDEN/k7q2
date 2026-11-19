"""Copy the decompiled sources that need a hook into patched/ and add the hook lines."""
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
# Inside the client tick where Forge calls a mod at its start and at its end: after the
# click delay has been counted down, and after everything else.
patch("net/minecraft/client/Minecraft.java", [
    ("            --this.rightClickDelayTimer;\n        }\n\n        this.mcProfiler.startSection(\"gui\");",
     "            --this.rightClickDelayTimer;\n        }\n\n        rig.Rig.clientPre(this);\n        this.mcProfiler.startSection(\"gui\");"),
    ("        this.mcProfiler.endSection();\n        this.systemTime = getSystemTime();",
     "        this.mcProfiler.endSection();\n        rig.Rig.clientPost(this);\n        this.systemTime = getSystemTime();"),
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
# A scene can ask for hits that do no damage to count, as CraftBukkit has them ("Don't filter
# out 0 damage"): a snowball, an egg, a rod's hook or a pearl then hurts a player for nothing.
patch("net/minecraft/entity/player/EntityPlayer.java", [
    ("                if (amount == 0.0F)\n                {\n                    return false;\n                }",
     "                if (amount == 0.0F && !rig.Rig.zeroHits)\n                {\n                    return false;\n                }"),
])
# The hook's random slowing where it lies on a block, noted so that a replay can use the same.
patch("net/minecraft/entity/projectile/EntityFishHook.java", [
    ("this.motionX *= (double)(this.rand.nextFloat() * 0.2F);", "this.motionX *= (double)(rig.Rig.hookFloat(this, this.rand.nextFloat()) * 0.2F);"),
    ("this.motionY *= (double)(this.rand.nextFloat() * 0.2F);", "this.motionY *= (double)(rig.Rig.hookFloat(this, this.rand.nextFloat()) * 0.2F);"),
    ("this.motionZ *= (double)(this.rand.nextFloat() * 0.2F);", "this.motionZ *= (double)(rig.Rig.hookFloat(this, this.rand.nextFloat()) * 0.2F);"),
])
# The random direction of a knockback from a thrower standing where the player is (a hook
# that falls back on its own angler), noted so that a replay can use the same.
patch("net/minecraft/entity/EntityLivingBase.java", [
    ("d0 = (Math.random() - Math.random()) * 0.01D)", "d0 = (rig.Rig.kbRandom(this, Math.random()) - rig.Rig.kbRandom(this, Math.random())) * 0.01D)"),
    ("d1 = (Math.random() - Math.random()) * 0.01D;", "d1 = (rig.Rig.kbRandom(this, Math.random()) - rig.Rig.kbRandom(this, Math.random())) * 0.01D;"),
])
# No chickens from eggs, as on duel servers: the rig keeps only the players and what they throw.
patch("net/minecraft/entity/projectile/EntityEgg.java", [
    ("if (!this.worldObj.isRemote && this.rand.nextInt(8) == 0)", "if (!this.worldObj.isRemote && this.rand.nextInt(8) == 0 && !rig.Rig.noChickens)"),
])
print("patched")
