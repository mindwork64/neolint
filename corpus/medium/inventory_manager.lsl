integer gChannel = -7001;
key gOwner;
list gItems;
integer gListener;

string itemName(integer index)
{
    return llGetInventoryName(INVENTORY_OBJECT, index);
}

integer itemCount()
{
    return llGetInventoryNumber(INVENTORY_OBJECT);
}

integer findItem(string name)
{
    integer i;
    for (i = 0; i < itemCount(); i++)
    {
        if (itemName(i) == name) return i;
    }
    return -1;
}

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        gItems = [];
        integer i;
        for (i = 0; i < itemCount(); i++)
        {
            gItems = gItems + [itemName(i)];
        }
        gListener = llListen(gChannel, "", gOwner, "");
    }

    touch_start(integer total)
    {
        llDialog(gOwner, "Inventory", ["Give", "Refresh"], gChannel);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "Give")
        {
            integer i;
            for (i = 0; i < itemCount(); i++) llGiveInventory(gOwner, itemName(i));
        }
        else if (message == "Refresh")
        {
            state ready;
        }
    }
}

state ready
{
    state_entry()
    {
        llOwnerSay("inventory ready: " + (string)llGetListLength(gItems));
    }

    touch_start(integer total)
    {
        state default;
    }
}
