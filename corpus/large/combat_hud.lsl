key gOwner;
key gTarget;
integer gHealth = 100;
integer gMana = 50;
integer gScore;
integer gChannel = -14001;
integer gListener;
list gMoves = ["Strike", "Guard", "Retreat"];

integer clampHealth(integer value)
{
    if (value < 0) return 0;
    if (value > 100) return 100;
    return value;
}

string healthLabel()
{
    return "HP " + (string)gHealth + " / MP " + (string)gMana;
}

refreshHud()
{
    llSetText(healthLabel() + " Score " + (string)gScore, <1.0, 1.0, 1.0>, 1.0);
}

key nearestTarget()
{
    list agents = llGetAgentList(AGENT_LIST_REGION, []);
    integer count = llGetListLength(agents);
    key best = NULL_KEY;
    float bestDistance = 9999.0;
    integer i;
    for (i = 0; i < count; i++)
    {
        key candidate = llList2Key(agents, i);
        if (candidate != gOwner)
        {
            vector position = llList2Vector(llGetObjectDetails(candidate, [OBJECT_POS]), 0);
            float distance = llVecDist(llGetPos(), position);
            if (distance < bestDistance)
            {
                bestDistance = distance;
                best = candidate;
            }
        }
    }
    return best;
}

applyDamage(integer amount)
{
    gHealth = clampHealth(gHealth - amount);
    if (gHealth == 0) state defeated;
    refreshHud();
}

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        gListener = llListen(gChannel, "", gOwner, "");
        llSetTimerEvent(1.0);
        refreshHud();
    }

    touch_start(integer total)
    {
        gTarget = nearestTarget();
        if (gTarget != NULL_KEY) llDialog(gOwner, "Choose a move", gMoves, gChannel);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "Strike")
        {
            gMana -= 5;
            gScore += 10;
            applyDamage(2);
        }
        else if (message == "Guard")
        {
            gHealth = clampHealth(gHealth + 3);
        }
        else if (message == "Retreat") state defeated;
        refreshHud();
    }

    timer()
    {
        gMana = gMana + 1;
        if (gMana > 100) gMana = 100;
        refreshHud();
    }
}

state defeated
{
    state_entry()
    {
        llSetTimerEvent(0.0);
        llSetText("Defeated", <1.0, 0.0, 0.0>, 1.0);
        llInstantMessage(gOwner, "You were defeated");
    }

    touch_start(integer total)
    {
        state default;
    }
}
