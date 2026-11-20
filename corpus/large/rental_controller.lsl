key gOwner;
integer gChannel = -13001;
integer gListener;
integer gExpiry;
list gTenants;
list gRates = ["Daily", "Weekly", "Monthly"];

integer isTenant(key id)
{
    integer i;
    for (i = 0; i < llGetListLength(gTenants); i++)
    {
        if (llList2Key(gTenants, i) == id) return TRUE;
    }
    return FALSE;
}

string tenantLabel(integer index)
{
    key id = llList2Key(gTenants, index);
    return llKey2Name(id) + " (" + (string)id + ")";
}

string formatRemaining(integer seconds)
{
    if (seconds <= 0) return "expired";
    integer minutes = seconds / 60;
    if (minutes < 60) return (string)minutes + " minutes";
    return (string)(minutes / 60) + " hours";
}

announce(string message)
{
    llInstantMessage(gOwner, message);
}

refreshHud()
{
    llSetText("Tenants: " + (string)llGetListLength(gTenants), <1.0, 1.0, 0.0>, 1.0);
}

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        gListener = llListen(gChannel, "", gOwner, "");
        gExpiry = llGetUnixTime() + 3600;
        llSetTimerEvent(60.0);
        refreshHud();
    }

    touch_start(integer total)
    {
        key id = llDetectedKey(0);
        if (!isTenant(id))
        {
            gTenants = gTenants + [id];
            llDialog(id, "Rental period", gRates, gChannel);
            refreshHud();
        }
        else
        {
            llInstantMessage(id, "Rental active: " + formatRemaining(gExpiry - llGetUnixTime()));
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "Monthly")
        {
            gExpiry = llGetUnixTime() + 2592000;
            llGiveInventory(id, "Rental Agreement");
            announce("Monthly rental started for " + llKey2Name(id));
        }
        else if (message == "Weekly")
        {
            gExpiry = llGetUnixTime() + 604800;
            llGiveInventory(id, "Rental Agreement");
        }
    }

    timer()
    {
        if (llGetUnixTime() >= gExpiry)
        {
            state vacant;
        }
        refreshHud();
    }
}

state vacant
{
    state_entry()
    {
        llSetTimerEvent(0.0);
        llSetText("Vacant", <1.0, 0.0, 0.0>, 1.0);
        announce("Rental is vacant");
    }

    touch_start(integer total)
    {
        state default;
    }
}
