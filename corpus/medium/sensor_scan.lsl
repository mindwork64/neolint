integer gScanCount;
list gAvatars;
float gRange = 10.0;
key gTarget;

integer closest()
{
    integer count = llGetListLength(gAvatars);
    integer best = -1;
    float bestDistance = 9999.0;
    vector here = llGetPos();
    integer i;
    for (i = 0; i < count; i++)
    {
        key id = llList2Key(gAvatars, i);
        vector position = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
        float distance = llVecDist(here, position);
        if (distance < bestDistance)
        {
            bestDistance = distance;
            best = i;
        }
    }
    return best;
}

default
{
    state_entry()
    {
        llSetTimerEvent(2.0);
        llSensor("", NULL_KEY, AGENT, gRange, PI);
    }

    sensor(integer total)
    {
        gAvatars = [];
        integer i;
        for (i = 0; i < total; i++)
        {
            gAvatars = gAvatars + [llDetectedKey(i)];
        }
        gScanCount++;
        llOwnerSay("seen " + (string)gScanCount);
    }

    no_sensor()
    {
        gAvatars = [];
        gTarget = NULL_KEY;
    }

    timer()
    {
        integer index = closest();
        if (index >= 0)
        {
            gTarget = llList2Key(gAvatars, index);
            llSetObjectDesc(llKey2Name(gTarget));
        }
        llSensor("", NULL_KEY, AGENT, gRange, PI);
    }
}
